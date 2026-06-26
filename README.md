<!-- SPDX-License-Identifier: MPL-2.0 -->

# treesitter

Tree-sitter parsing and syntax analysis for the [Wippy runtime](https://github.com/wippyai/runtime), delivered as a self-contained WebAssembly module — no CGO, no native build tag.

A Rust component compiles the tree-sitter C runtime and its grammars to `wasm32-wasip2` and runs resident inside the runtime's wazero engine. Each parse keeps its syntax tree in the component's linear memory keyed by an integer handle; `parse` returns the whole tree serialized once, and `Node`/`Tree`/`Cursor` navigation runs in pure Lua over that snapshot. Only `parse`, `query`, `language`, `edit`, and `free` cross into WebAssembly.

## Supported languages

`go` (`golang`), `javascript` (`js`), `typescript` (`ts`), `tsx`, `python` (`py`), `php`, `c#` (`csharp`, `cs`), `html` (`html5`), `lua`, `markdown` (`md`), `sql`.

## Usage

```lua
local treesitter = require("treesitter")

local tree = treesitter.parse("go", [[
package main

func hello() string {
    return "Hello, World!"
}
]])

local root = tree:root_node()
print(root:kind())  -- "source_file"

local query = treesitter.query("go", [[
    (function_declaration name: (identifier) @func_name)
]])

for _, capture in ipairs(query:captures(root, code)) do
    print(capture.name, capture.text)  -- "func_name", "hello"
end

local cursor = tree:walk()
cursor:goto_first_child()
print(cursor:current_node():kind())

cursor:close()
query:close()
tree:close()
```

Errors are structured: check `err:kind()` against `errors.INVALID` / `errors.INTERNAL`.

## Building

```bash
make image            # build the wasi-sdk + rust image (once)
make build-component  # compile the Rust WASM engine in docker, stage it, inject its sha256
make wippy            # point ./wippy at ../runtime/dist/wippy-<os>-<arch>
make test             # run the test suites against the runtime
make check            # build-component + lint + test
```

The component is built reproducibly inside Docker (wasi-sdk + rustup), so a host C/Rust toolchain is not required.
