local test = require("test")
local treesitter = require("treesitter")

local GO = [[
package main

func hello() string {
    return "Hello, World!"
}
]]

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

        test.it("reports supported languages", function()
            local langs = treesitter.supported_languages()
            test.eq(langs.go, true)
        end)
    end)
end

local run_cases = test.run_cases(define)

local function run(options)
    return run_cases(options)
end

return { run = run }
