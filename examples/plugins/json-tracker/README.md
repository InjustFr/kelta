# json-tracker

A process (KPP) provider plugin (PLUGINS.md §9): a tracker whose tickets live in a JSON file, served by
`tracker.cjs` (Node, no dependencies) over JSON-RPC on stdio. Check it with
`cargo run -p xtask -- kpp-check examples/plugins/json-tracker` (it writes to the data file: run it on a copy).
