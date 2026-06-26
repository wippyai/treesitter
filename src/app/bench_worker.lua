local funcs = require("funcs")
local json = require("json")

local function run(count: number, code: string): number
    local body = json.encode({ op = "extract", language = "go", code = code })
    local total = 0
    for _ = 1, count do
        local out = funcs.call("treesitter:engine_pool", body)
        local decoded = json.decode(out :: string)
        total = total + ((decoded :: any).n or 0)
    end
    return total
end

return { run = run }
