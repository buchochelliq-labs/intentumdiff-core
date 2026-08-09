let
    Source = Sql.Database("server", "sales"),
    FilteredRows = Table.SelectRows(Source, each [Amount] > 100),
    TaggedRows = Table.AddColumn(FilteredRows, "Region", each "emea"),
    RenamedColumns = Table.RenameColumns(TaggedRows, {{"Amount", "OrderAmount"}}),
    UnusedPreview = Table.FirstN(Source, 5)
in
    RenamedColumns
