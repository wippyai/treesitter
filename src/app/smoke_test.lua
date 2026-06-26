local test = require("test")
local treesitter = require("treesitter")

local GO = [[
package main

func hello() string {
    return "Hello, World!"
}
]]

local ROOT_KIND = {
    { lang = "go", code = "package main\n", root = "source_file" },
    { lang = "golang", code = "package main\n", root = "source_file" },
    { lang = "javascript", code = "const x = 1;\n", root = "program" },
    { lang = "js", code = "const x = 1;\n", root = "program" },
    { lang = "typescript", code = "const x: number = 1;\n", root = "program" },
    { lang = "tsx", code = "const x = 1;\n", root = "program" },
    { lang = "python", code = "x = 1\n", root = "module" },
    { lang = "py", code = "x = 1\n", root = "module" },
    { lang = "php", code = "<?php $x = 1;\n", root = "program" },
    { lang = "csharp", code = "class A {}\n", root = "compilation_unit" },
    { lang = "html", code = "<html></html>\n", root = "document" },
    { lang = "lua", code = "local x = 1\n", root = "chunk" },
}

local function define()
    test.describe("treesitter.smoke", function()
        test.it("parses Go and exposes the root node", function()
            local tree, err = treesitter.parse("go", GO)
            test.eq(err, nil)
            local root = tree:root_node()
            test.eq(root:kind(), "source_file")
            test.eq(root:type(), "source_file")
            test.eq(root:start_byte(), 0)
            test.eq(root:has_error(), false)
            tree:close()
        end)

        test.it("navigates to a function declaration and reads its name", function()
            local tree = treesitter.parse("go", GO)
            local root = tree:root_node()
            local found_name = nil
            for i = 0, root:named_child_count() - 1 do
                local child = root:named_child(i)
                if child:kind() == "function_declaration" then
                    local name = child:child_by_field_name("name")
                    if name ~= nil then
                        found_name = name:text()
                    end
                end
            end
            test.eq(found_name, "hello")
            tree:close()
        end)

        test.it("parses every supported language to its expected root kind", function()
            for _, case in ipairs(ROOT_KIND) do
                local tree, err = treesitter.parse(case.lang, case.code)
                test.eq(err, nil)
                test.eq(tree:root_node():kind(), case.root)
                tree:close()
            end
        end)

        test.it("reports supported languages by canonical name", function()
            local langs = treesitter.supported_languages()
            test.eq(langs.go, true)
            test.eq(langs.javascript, true)
            test.eq(langs.typescript, true)
            test.eq(langs["typescript+jsx"], true)
            test.eq(langs.python, true)
            test.eq(langs.php, true)
            test.eq(langs["c#"], true)
            test.eq(langs.html, true)
            test.eq(langs.lua, true)
            test.eq(langs.golang, nil)
        end)

        test.it("rejects an unsupported language with an error", function()
            local tree, err = treesitter.parse("cobol", "IDENTIFICATION DIVISION.")
            test.eq(tree, nil)
            test.eq(err == nil, false)
        end)
    end)
end

local run_cases = test.run_cases(define)

local function run(options)
    return run_cases(options)
end

return { run = run }
