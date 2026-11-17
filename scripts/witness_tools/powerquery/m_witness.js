// #262 Power Query M witness: Microsoft's own @microsoft/powerquery-parser, outside
// IntentumDiff. For each file: lex + parse as the upstream lexParseResource test does
// (TaskUtils.tryLexParse with DefaultSettings), report the parse stage outcome and, for a
// section document, every section member's start (UTF-16 code-unit offset into the text
// with a leading BOM removed). Usage: node m_witness.js FILE... (module resolved via NODE_PATH).
const fs = require("fs");
const PQP = require("@microsoft/powerquery-parser");

async function main() {
    const out = { package: "@microsoft/powerquery-parser", version: require("@microsoft/powerquery-parser/package.json").version,
                  node: process.version, results: [] };
    for (const path of process.argv.slice(2)) {
        let text = fs.readFileSync(path, "utf8");
        const bom = text.charCodeAt(0) === 0xfeff;
        if (bom) text = text.slice(1);
        const task = await PQP.TaskUtils.tryLexParse(PQP.DefaultSettings, text);
        const ok = PQP.TaskUtils.isParseStageOk(task);
        const row = { path, bom, parse_ok: ok, stage: task.stage, result_kind: task.resultKind };
        if (!ok) row.error = String(task.error && task.error.message).slice(0, 500);
        else {
            const root = task.ast;
            row.root_kind = root.kind;
            if (root.kind === PQP.Language.Ast.NodeKind.Section) {
                row.members = root.sectionMembers.elements.map(m => {
                    const tokens = task.lexerSnapshot.tokens; const t = tokens[m.tokenRange.tokenIndexStart];
                    return { offset: t.positionStart.codeUnit, name: m.namePairedExpression.key.literal };
                });
            }
        }
        out.results.push(row);
    }
    console.log(JSON.stringify(out));
}
main().catch(e => { console.error(e); process.exit(2); });
