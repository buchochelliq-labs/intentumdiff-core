// #262 T-SQL witness. For each file: parse with TSql160Parser and TSql150Parser
// (initialQuotedIdentifiers: true, as the upstream ParserTest harness), report error
// counts, every statement's character offset/length (batches flattened, in order), and
// the Sql160ScriptGenerator rendering of the parse tree. Output: one JSON document.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using Microsoft.SqlServer.TransactSql.ScriptDom;

static class Program
{
    static object Parse(TSqlParser parser, string text, out TSqlFragment tree)
    {
        using var reader = new StringReader(text);
        tree = parser.Parse(reader, out IList<ParseError> errors);
        return errors.Select(e => new { e.Number, e.Offset, e.Line, e.Column, e.Message }).ToList();
    }

    static int Main(string[] args)
    {
        var results = new List<object>();
        foreach (var path in args)
        {
            var text = File.ReadAllText(path); // strips a UTF-8 BOM; offsets are UTF-16 indexes into this text
            var e160 = (System.Collections.IList)Parse(new TSql160Parser(true), text, out var tree);
            var e150 = (System.Collections.IList)Parse(new TSql150Parser(true), text, out _);
            var statements = new List<object>();
            string generated = null;
            if (tree is TSqlScript script)
            {
                foreach (var batch in script.Batches)
                    foreach (var st in batch.Statements)
                        statements.Add(new { offset = st.StartOffset, length = st.FragmentLength, type = st.GetType().Name });
                if (e160.Count == 0)
                {
                    new Sql160ScriptGenerator().GenerateScript(tree, out generated);
                }
            }
            results.Add(new { path, bom_stripped_text_length = text.Length, errors160 = e160, errors150 = e150,
                              statements, generated160 = generated });
        }
        Console.WriteLine(JsonSerializer.Serialize(new {
            parser_assembly = typeof(TSql160Parser).Assembly.GetName().Version?.ToString(),
            parser_informational = typeof(TSql160Parser).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false)
                .Cast<System.Reflection.AssemblyInformationalVersionAttribute>().FirstOrDefault()?.InformationalVersion,
            runtime = Environment.Version.ToString(), results }));
        return 0;
    }
}
