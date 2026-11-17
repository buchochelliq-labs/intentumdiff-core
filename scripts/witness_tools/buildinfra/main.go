// Command buildinfra is the reference-parser witness tool for the #267 build,
// configuration and infrastructure family (core #268 ledger). It wraps the upstream
// parsers themselves, never IntentumDiff:
//
//	dockerfile FILE  BuildKit frontend/dockerfile/parser.Parse + instructions.Parse
//	hcl FILE         hashicorp/hcl/v2 hclsyntax.ParseConfig (native syntax)
//	modfile FILE     golang.org/x/mod/modfile.Parse (strict) + Format
//	hclfmt FILE      hclwrite.Format (the formatter behind terraform fmt); prints the bytes
//
// Each prints one JSON object on stdout: whether the reference accepts the document,
// its diagnostics, the start byte of each top-level unit and a normalised semantic
// projection used to compare an old and a new document.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/hashicorp/hcl/v2"
	"github.com/hashicorp/hcl/v2/hclsyntax"
	"github.com/hashicorp/hcl/v2/hclwrite"
	"github.com/moby/buildkit/frontend/dockerfile/instructions"
	"github.com/moby/buildkit/frontend/dockerfile/parser"
	ctyjson "github.com/zclconf/go-cty/cty/json"
	"golang.org/x/mod/modfile"
)

type out struct {
	Tool       string      `json:"tool"`
	Accepts    bool        `json:"accepts"`
	Error      string      `json:"error,omitempty"`
	Starts     []int       `json:"unit_starts"`
	Projection interface{} `json:"projection"`
	Extra      interface{} `json:"extra,omitempty"`
}

func lineStarts(src []byte) []int {
	starts := []int{0}
	for i, b := range src {
		if b == '\n' {
			starts = append(starts, i+1)
		}
	}
	return starts
}

func strip(v interface{}) interface{} {
	switch t := v.(type) {
	case map[string]interface{}:
		m := map[string]interface{}{}
		for k, x := range t {
			switch k {
			case "Location", "SourceCode", "OrigCmd", "Comment", "NoDelim":
				continue
			}
			m[k] = strip(x)
		}
		return m
	case []interface{}:
		for i := range t {
			t[i] = strip(t[i])
		}
		return t
	}
	return v
}

func typed(v interface{}) interface{} {
	raw, err := json.Marshal(v)
	if err != nil {
		return map[string]string{"marshal_error": err.Error()}
	}
	var g interface{}
	_ = json.Unmarshal(raw, &g)
	return strip(g)
}

func dockerfile(src []byte) out {
	o := out{Tool: "buildkit frontend/dockerfile/parser + instructions"}
	res, err := parser.Parse(bytes.NewReader(src))
	if err != nil {
		o.Error = err.Error()
		return o
	}
	o.Accepts = true
	ls := lineStarts(src)
	for _, n := range res.AST.Children {
		off := ls[n.StartLine-1]
		for off < len(src) && (src[off] == ' ' || src[off] == '\t') {
			off++
		}
		o.Starts = append(o.Starts, off)
	}
	stages, meta, perr := instructions.Parse(res.AST, nil)
	proj := map[string]interface{}{"dump": res.AST.Dump() + "\n", "escape_token": string(res.EscapeToken)}
	if perr != nil {
		proj["instructions_error"] = perr.Error()
	} else {
		var st []interface{}
		for _, s := range stages {
			var cmds []interface{}
			for _, c := range s.Commands {
				// The instruction keyword is dispatched case-insensitively; its spelling is not observed.
				cmds = append(cmds, map[string]interface{}{"type": fmt.Sprintf("%T", c), "fields": typed(c)})
			}
			st = append(st, map[string]interface{}{"name": s.Name, "base": s.BaseName, "platform": s.Platform, "commands": cmds})
		}
		proj["meta_args"] = typed(meta)
		proj["stages"] = st
	}
	o.Projection = proj
	var warns []string
	for _, w := range res.Warnings {
		warns = append(warns, w.Short)
	}
	o.Extra = map[string]interface{}{"warnings": warns}
	return o
}

func tokensText(src []byte, r hcl.Range) string {
	toks, _ := hclsyntax.LexExpression(src[r.Start.Byte:r.End.Byte], "expr", hcl.Pos{Line: 1, Column: 1})
	var parts []string
	for _, t := range toks {
		switch t.Type {
		case hclsyntax.TokenComment, hclsyntax.TokenNewline, hclsyntax.TokenEOF:
			continue
		}
		parts = append(parts, string(t.Bytes))
	}
	return strings.Join(parts, " ")
}

func hclBody(src []byte, b *hclsyntax.Body) map[string]interface{} {
	attrs := map[string]interface{}{}
	for name, a := range b.Attributes {
		v, diags := a.Expr.Value(nil)
		if !diags.HasErrors() && v.IsWhollyKnown() {
			raw, err := ctyjson.Marshal(v, v.Type())
			ty, _ := ctyjson.MarshalType(v.Type())
			if err == nil {
				attrs[name] = map[string]interface{}{"value": json.RawMessage(raw), "type": json.RawMessage(ty)}
				continue
			}
		}
		attrs[name] = map[string]interface{}{"expr_tokens": tokensText(src, a.Expr.Range())}
	}
	var blocks []interface{}
	for _, bl := range b.Blocks {
		blocks = append(blocks, map[string]interface{}{"type": bl.Type, "labels": bl.Labels, "body": hclBody(src, bl.Body)})
	}
	return map[string]interface{}{"attributes": attrs, "blocks": blocks}
}

func hclDoc(src []byte) out {
	o := out{Tool: "hashicorp/hcl/v2 hclsyntax.ParseConfig"}
	f, diags := hclsyntax.ParseConfig(src, "fixture.hcl", hcl.Pos{Line: 1, Column: 1, Byte: 0})
	if diags.HasErrors() {
		o.Error = diags.Error()
		return o
	}
	o.Accepts = true
	body := f.Body.(*hclsyntax.Body)
	var starts []int
	for _, a := range body.Attributes {
		starts = append(starts, a.SrcRange.Start.Byte)
	}
	for _, bl := range body.Blocks {
		starts = append(starts, bl.TypeRange.Start.Byte)
	}
	sort.Ints(starts)
	o.Starts = starts
	o.Projection = hclBody(src, body)
	return o
}

func modDoc(src []byte) out {
	o := out{Tool: "golang.org/x/mod/modfile.Parse"}
	f, err := modfile.Parse("go.mod", src, nil)
	if err != nil {
		o.Error = err.Error()
		return o
	}
	o.Accepts = true
	for _, st := range f.Syntax.Stmt {
		start, _ := st.Span()
		o.Starts = append(o.Starts, start.Byte)
	}
	o.Projection = map[string]interface{}{"format": string(modfile.Format(f.Syntax))}
	return o
}

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: buildinfra dockerfile|hcl|modfile FILE")
		os.Exit(2)
	}
	src, err := os.ReadFile(os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	if os.Args[1] == "hclfmt" {
		os.Stdout.Write(hclwrite.Format(src))
		return
	}
	var o out
	switch os.Args[1] {
	case "dockerfile":
		o = dockerfile(src)
	case "hcl":
		o = hclDoc(src)
	case "modfile":
		o = modDoc(src)
	default:
		os.Exit(2)
	}
	if o.Starts == nil {
		o.Starts = []int{}
	}
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", " ")
	_ = enc.Encode(o)
}
