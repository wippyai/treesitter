local treesitter = require("treesitter")
local funcs = require("funcs")
local json = require("json")
local time = require("time")

local CHUNK = [[
func Name%d(a int, b string) (int, error) {
    if a > 0 {
        return a + len(b), nil
    }
    return 0, fmt.Errorf("bad %s", b)
}
]]

local function gen_go(target_bytes: number): string
    local parts = { "package main\n\nimport \"fmt\"\n\n" }
    local size = #parts[1]
    local i = 0
    while size < target_bytes do
        i = i + 1
        local s = string.format(CHUNK, i, "x")
        parts[#parts + 1] = s
        size = size + #s
    end
    return table.concat(parts)
end

local function now_ns(): number
    return time.now():unix_nano()
end

local function bench(label: string, code: string, iters: number)
    local warm = treesitter.parse("go", code)
    local nodes = warm.n
    warm:close()

    local best = math.huge
    for _ = 1, iters do
        local a = now_ns()
        local tr = treesitter.parse("go", code)
        local b = now_ns()
        local ms = (b - a) / 1e6
        if ms < best then
            best = ms
        end
        tr:close()
    end

    local tr = treesitter.parse("go", code)
    local visited = 0
    local wa = now_ns()
    local cursor = tr:walk()
    local function descend()
        visited = visited + 1
        if cursor:goto_first_child() then
            repeat
                descend()
            until not cursor:goto_next_sibling()
            cursor:goto_parent()
        end
    end
    descend()
    local wb = now_ns()

    local qa = now_ns()
    local query = treesitter.query("go", "(function_declaration name: (identifier) @n)")
    local caps = query:captures(tr:root_node(), code)
    local qb = now_ns()
    query:close()
    tr:close()

    print(string.format(
        "%-7s | %8d B | %7d nodes | parse %7.2f ms | walk %6.2f ms (%d) | query %6.2f ms (%d caps) | %.1f MB/s",
        label, #code, nodes, best, (wb - wa) / 1e6, visited, (qb - qa) / 1e6, #caps,
        (#code / 1024 / 1024) / (best / 1000)
    ))
end

local function probe(label: string, code: string)
    local body = json.encode({ op = "parse", language = "go", code = code })
    local best_call = math.huge
    local best_decode = math.huge
    local out: any = nil
    for _ = 1, 5 do
        local a = now_ns()
        local raw = funcs.call("treesitter:engine", body)
        local b = now_ns()
        out = raw
        local decoded = json.decode(raw :: string)
        local c = now_ns()
        best_call = math.min(best_call, (b - a) / 1e6)
        best_decode = math.min(best_decode, (c - b) / 1e6)
        funcs.call("treesitter:engine", json.encode({ op = "free", handle = decoded.handle }))
    end
    print(string.format("PROBE %-7s | json=%d B | funcs.call(parse+serialize+marshal)=%6.2f ms | json.decode=%6.2f ms",
        label, #(out :: string), best_call, best_decode))
end

local function run()
    print("BENCH-START treesitter wasm engine (go grammar)")
    local body = json.encode({ op = "languages" })
    local floor = math.huge
    for _ = 1, 300 do
        local a = now_ns()
        funcs.call("treesitter:engine", body)
        local b = now_ns()
        floor = math.min(floor, (b - a) / 1e6)
    end
    print(string.format("FLOOR minimal funcs.call round-trip = %.4f ms", floor))

    local code = gen_go(6656)

    local function concurrent(workers: number, total: number): (number, number)
        local events = process.events()
        local per = math.floor(total / workers)
        local t = now_ns()
        for _ = 1, workers do
            process.spawn_monitored("app:bench_worker", "app:procs", per, code)
        end
        local done = 0
        while done < workers do
            local sel = channel.select({ events:case_receive() })
            if sel.value ~= nil and sel.value.kind == process.event.EXIT then
                done = done + 1
            end
        end
        return (now_ns() - t) / 1e6, per * workers
    end

    local TOTAL = 800
    local s1, f1 = concurrent(1, TOTAL)
    print(string.format("CONC sequential (1 worker)  | %d files (avg %d B) | %.0f ms (%.3f ms/file)", f1, #code, s1, s1 / f1))
    local s8, f8 = concurrent(8, TOTAL)
    print(string.format("CONC parallel   (8 workers) | %d files | %.0f ms (%.3f ms/file) | speedup %.1fx", f8, s8, s8 / f8, s1 / s8))

    local raw = funcs.call("treesitter:engine", json.encode({ op = "stats" }))
    local stats: any = json.decode(raw :: string)
    print(string.format("STATS resident trees after run = %d (handle counter %d)", stats.trees, stats.next))
    print("BENCH-END")
    return true
end

return { run = run }
