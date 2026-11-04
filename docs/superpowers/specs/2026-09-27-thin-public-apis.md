# Shared engine, two public APIs

User-approved objective: complete the thin-wrapper migration, following the ownership model of rs-rich-cli. Rust and Python are separately supported public APIs. Future Go/Java APIs must use the same implementation. The existing C ABI remains the only language boundary; public users receive idiomatic types and errors. No packaging rewrite, merge or release is part of this work.

Rust owns shared semantic decisions, tree processing, profile/schema interpretation, Markdown reconciliation, lifecycle classification and compile-context interpretation. Python owns host I/O, callbacks, serialization and DTO conversion. A missing or failing engine must not activate a second semantic implementation.

Existing owner issues: core97,98,99,55,100; core101 covers output validity and corpus; Python54/57 cover the adapter and public API. Existing catalogue/trust/config/native CLI issues39–42 remain tracked and must not be confused with completed migrations.

For each major change, inspect actual before/after diff output, compare native and Python results on identical data, and independently judge expected changes. A shared wrong answer is not parity success. Minimized fixtures assert required changes, forbidden changes, source positions, groups and flags; defects require failing regression evidence. An independent reviewer must confirm implementation and actual output before readiness.
