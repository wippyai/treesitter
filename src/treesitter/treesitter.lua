local funcs = require("funcs")
local json = require("json")

local ENGINE = "treesitter:engine"

local NAMES = {
    { name = "lua", aliases = { "lua" } },
    { name = "php", aliases = { "php" } },
    { name = "go", aliases = { "go", "golang" } },
    { name = "javascript", aliases = { "js", "javascript" } },
    { name = "typescript+jsx", aliases = { "tsx" } },
    { name = "typescript", aliases = { "ts", "typescript" } },
    { name = "python", aliases = { "python", "py" } },
    { name = "c#", aliases = { "csharp", "c#", "cs" } },
    { name = "html", aliases = { "html", "html5" } },
}

local ALIAS_TO_NAME = {}
for _, entry in ipairs(NAMES) do
    for _, alias in ipairs(entry.aliases) do
        ALIAS_TO_NAME[alias] = entry.name
    end
end

local FLAG_NAMED = 1
local FLAG_EXTRA = 2
local FLAG_MISSING = 4
local FLAG_ERROR = 8
local FLAG_HAS_ERROR = 16

local function fail(message: any, kind: any?): any
    return errors.new({
        message = message,
        kind = kind or errors.INVALID,
        retryable = false,
    })
end

local function call(req: any): (any, any?)
    local body = json.encode(req)
    local out, err = funcs.call(ENGINE, body)
    if err ~= nil then
        return nil, tostring(err)
    end
    local decoded = json.decode(out :: string)
    if type(decoded) == "table" and decoded.error ~= nil then
        return nil, decoded.error
    end
    return decoded, nil
end

local function fetch_record(tree: any, id: any): any
    local r, err = call({ op = "node", handle = tree.handle, id = id })
    if err ~= nil or r == nil then
        error("treesitter: node fetch failed: " .. tostring(err), 2)
    end
    return r
end

local function point_le(a: any, b: any): boolean
    if a.row ~= b.row then
        return a.row < b.row
    end
    return a.column <= b.column
end

local function flagged(record: any, bit: any): boolean
    return math.floor(record.flags / bit) % 2 >= 1
end

local function materialize(tree: any): any
    if tree.cols == nil then
        local cols, err = call({ op = "columns", handle = tree.handle })
        if err ~= nil or cols == nil then
            error("treesitter: columns fetch failed: " .. tostring(err), 2)
        end
        cols.gn = cols.gn or {}
        cols.fields = cols.fields or {}
        cols.field_ids = cols.field_ids or {}
        tree.cols = cols
    end
    return tree.cols
end

local function record_from_cols(cols: any, i: any): any
    local off = cols.ch_off[i + 1]
    local stop = cols.ch_off[i + 2]
    local children = {}
    local named = {}
    for j = off + 1, stop do
        local cid = cols.ch_val[j]
        children[#children + 1] = cid
        if math.floor(cols.flags[cid + 1] / FLAG_NAMED) % 2 >= 1 then
            named[#named + 1] = cid
        end
    end
    local r = {
        kind = cols.kinds[cols.k[i + 1] + 1],
        flags = cols.flags[i + 1],
        sb = cols.sb[i + 1],
        eb = cols.eb[i + 1],
        sr = cols.sr[i + 1],
        sc = cols.sc[i + 1],
        er = cols.er[i + 1],
        ec = cols.ec[i + 1],
        parent = cols.parent[i + 1],
        dc = cols.dc[i + 1],
        children = children,
        named_children = named,
        fields = cols.fields[tostring(i)],
        field_ids = cols.field_ids[tostring(i)],
    }
    local g = cols.gn[tostring(i)]
    if g ~= nil then
        r.gn = cols.kinds[g + 1]
    end
    return r
end

local function col_child_count(cols: any, i: any): any
    return cols.ch_off[i + 2] - cols.ch_off[i + 1]
end

local function col_child(cols: any, i: any, index: any): any
    local off = cols.ch_off[i + 1]
    if index < 0 or index >= cols.ch_off[i + 2] - off then
        return nil
    end
    return cols.ch_val[off + index + 1]
end

local function col_position(cols: any, i: any): (any, any)
    local p = cols.parent[i + 1]
    if p < 0 then
        return nil, nil
    end
    local off = cols.ch_off[p + 1]
    local count = cols.ch_off[p + 2] - off
    for pos = 0, count - 1 do
        if cols.ch_val[off + pos + 1] == i then
            return p, pos
        end
    end
    return nil, nil
end

local Node = {}
Node.__index = Node

local function wrap_node(tree: any, id: any): any
    if id == nil or id < 0 then
        return nil
    end
    return setmetatable({ tree = tree, i = id, record = nil }, Node)
end

local function rec(self: any): any
    if self.record == nil then
        if self.tree.cols ~= nil then
            self.record = record_from_cols(self.tree.cols, self.i)
        else
            self.record = fetch_record(self.tree, self.i)
        end
    end
    return self.record
end

function Node:kind()
    return rec(self).kind
end

Node.type = Node.kind

function Node:grammar_name()
    local r = rec(self)
    return r.gn or r.kind
end

function Node:is_named()
    return flagged(rec(self), FLAG_NAMED)
end

function Node:is_extra()
    return flagged(rec(self), FLAG_EXTRA)
end

function Node:is_missing()
    return flagged(rec(self), FLAG_MISSING)
end

function Node:is_error()
    return flagged(rec(self), FLAG_ERROR)
end

function Node:has_error()
    return flagged(rec(self), FLAG_HAS_ERROR)
end

function Node:start_byte()
    return rec(self).sb + self.tree.byte_offset
end

function Node:end_byte()
    return rec(self).eb + self.tree.byte_offset
end

local function offset_point(self: any, row: any, column: any): any
    local r = row + self.tree.row_offset
    local c = column
    if row == 0 then
        c = c + self.tree.column_offset
    end
    return { row = r, column = c }
end

function Node:start_point()
    local r = rec(self)
    return offset_point(self, r.sr, r.sc)
end

function Node:end_point()
    local r = rec(self)
    return offset_point(self, r.er, r.ec)
end

function Node:child_count()
    return #rec(self).children
end

function Node:child(index)
    return wrap_node(self.tree, rec(self).children[index + 1])
end

function Node:named_child_count()
    return #rec(self).named_children
end

function Node:named_child(index)
    return wrap_node(self.tree, rec(self).named_children[index + 1])
end

function Node:parent()
    local p = rec(self).parent
    if p == nil or p < 0 then
        return nil
    end
    return wrap_node(self.tree, p)
end

local function sibling_at(self: any, step: any, named_only: any): any
    local r = rec(self)
    if r.parent < 0 then
        return nil
    end
    local parent = wrap_node(self.tree, r.parent)
    local list = named_only and rec(parent).named_children or rec(parent).children
    for idx, id in ipairs(list) do
        if id == self.i then
            local sib = list[idx + step]
            if sib == nil then
                return nil
            end
            return wrap_node(self.tree, sib)
        end
    end
    return nil
end

function Node:next_sibling()
    return sibling_at(self, 1, false)
end

function Node:prev_sibling()
    return sibling_at(self, -1, false)
end

function Node:next_named_sibling()
    return sibling_at(self, 1, true)
end

function Node:prev_named_sibling()
    return sibling_at(self, -1, true)
end

function Node:child_by_field_name(name)
    local r = rec(self)
    local fields = r.fields
    if fields == nil then
        return nil
    end
    for pos = 0, #r.children - 1 do
        if fields[tostring(pos)] == name then
            return wrap_node(self.tree, r.children[pos + 1])
        end
    end
    return nil
end

function Node:field_name_for_child(index)
    local fields = rec(self).fields
    if fields == nil then
        return nil
    end
    return fields[tostring(index)]
end

function Node:descendant_count()
    return rec(self).dc
end

function Node:named_descendant_for_point_range(start_point, end_point)
    local res, err = call({
        op = "descendant",
        handle = self.tree.handle,
        node = self.i,
        start = { row = start_point.row, column = start_point.column },
        ["end"] = { row = end_point.row, column = end_point.column },
    })
    if err ~= nil or res == nil then
        return nil
    end
    return wrap_node(self.tree, res.node)
end

function Node:text(source)
    local code = source
    if code == nil then
        if self.tree.source == nil then
            return nil, fail("source reference is empty")
        end
        code = self.tree.source
    end
    local r = rec(self)
    if r.sb > r.eb or r.eb > #code then
        return nil, fail("invalid byte range")
    end
    return code:sub(r.sb + 1, r.eb)
end

function Node:to_sexp()
    local res = call({ op = "sexp", handle = self.tree.handle, node = self.i })
    if res == nil then
        return ""
    end
    return res.sexp
end

local Cursor = {}
Cursor.__index = Cursor

local function new_cursor(tree: any, start_id: any): any
    materialize(tree)
    return setmetatable({
        tree = tree,
        start = start_id,
        stack = { start_id },
        cache = {},
        closed = false,
    }, Cursor)
end

local function cursor_rec(self: any, id: any): any
    local r = self.cache[id]
    if r == nil then
        r = record_from_cols(self.tree.cols, id)
        self.cache[id] = r
    end
    return r
end

local function cursor_current(self: any): any
    return self.stack[#self.stack]
end

function Cursor:current_node()
    return wrap_node(self.tree, cursor_current(self))
end

function Cursor:current_depth()
    return #self.stack - 1
end

function Cursor:current_descendant_index()
    return cursor_current(self) - self.start
end

local function cursor_field_pos(self: any): (any, any)
    if #self.stack < 2 then
        return nil, nil
    end
    local cur = cursor_current(self)
    local parent = self.stack[#self.stack - 1]
    local children = cursor_rec(self, parent).children
    for idx, id in ipairs(children) do
        if id == cur then
            return parent, idx - 1
        end
    end
    return nil, nil
end

function Cursor:current_field_id()
    local parent, pos = cursor_field_pos(self)
    if parent == nil then
        return 0
    end
    local ids = cursor_rec(self, parent).field_ids
    if ids == nil then
        return 0
    end
    return ids[tostring(pos)] or 0
end

function Cursor:current_field_name()
    local parent, pos = cursor_field_pos(self)
    if parent == nil then
        return nil
    end
    local fields = cursor_rec(self, parent).fields
    if fields == nil then
        return nil
    end
    return fields[tostring(pos)]
end

function Cursor:goto_parent()
    if #self.stack <= 1 then
        return false
    end
    table.remove(self.stack :: { any })
    return true
end

function Cursor:goto_first_child()
    local children = cursor_rec(self, cursor_current(self)).children
    if #children == 0 then
        return false
    end
    table.insert(self.stack, children[1])
    return true
end

function Cursor:goto_last_child()
    local children = cursor_rec(self, cursor_current(self)).children
    if #children == 0 then
        return false
    end
    table.insert(self.stack, children[#children])
    return true
end

local function cursor_goto_sibling(self: any, step: any): boolean
    if #self.stack < 2 then
        return false
    end
    local cur = cursor_current(self)
    local parent = self.stack[#self.stack - 1]
    local children = cursor_rec(self, parent).children
    for idx, id in ipairs(children) do
        if id == cur then
            local sib = children[idx + step]
            if sib == nil then
                return false
            end
            self.stack[#self.stack] = sib
            return true
        end
    end
    return false
end

function Cursor:goto_next_sibling()
    return cursor_goto_sibling(self, 1)
end

function Cursor:goto_previous_sibling()
    return cursor_goto_sibling(self, -1)
end

function Cursor:goto_descendant(index)
    local target = self.start + index
    local path = {}
    local node = target
    while node ~= nil and node >= self.start do
        table.insert(path, 1, node)
        if node == self.start then
            break
        end
        node = cursor_rec(self, node).parent
    end
    self.stack = path
end

local function cursor_goto_first_child_for(self: any, predicate: any): any
    local children = cursor_rec(self, cursor_current(self)).children
    for idx, id in ipairs(children) do
        if predicate(cursor_rec(self, id)) then
            table.insert(self.stack, id)
            return idx - 1
        end
    end
    return nil
end

function Cursor:goto_first_child_for_byte(byte)
    return cursor_goto_first_child_for(self, function(r: any): any
        return r.eb > byte
    end)
end

function Cursor:goto_first_child_for_point(point)
    return cursor_goto_first_child_for(self, function(r: any): any
        return point_le(point, { row = r.er, column = r.ec })
    end)
end

function Cursor:reset(node)
    self.start = node.i
    self.stack = { node.i }
    self.cache = {}
end

function Cursor:reset_to(other)
    self.start = other.start
    self.stack = {}
    for _, id in ipairs(other.stack) do
        table.insert(self.stack, id)
    end
end

function Cursor:copy()
    local c = new_cursor(self.tree, self.start)
    c.stack = {}
    for _, id in ipairs(self.stack) do
        table.insert(c.stack, id)
    end
    return c
end

function Cursor:close()
    self.closed = true
end

local Query = {}
Query.__index = Query

local function build_query(language: any, pattern: any, meta: any): any
    return setmetatable({
        language = language,
        pattern = pattern,
        meta = meta,
        byte_range = nil,
        point_range = nil,
        match_limit = nil,
        max_start_depth = nil,
        disabled_patterns = {},
        disabled_captures = {},
        exceeded = false,
        closed = false,
    }, Query)
end

local function query_exec(self: any, node: any, source: any, mode: any): (any, any?)
    if type(node) ~= "table" or node.tree == nil then
        return nil, fail("Node expected")
    end
    local req = {
        op = "query",
        language = self.language,
        pattern = self.pattern,
        handle = node.tree.handle,
        node = node.i,
        source = source,
        mode = mode,
        disable_patterns = self.disabled_patterns,
        disable_captures = self.disabled_captures,
    }
    if self.byte_range ~= nil then
        req.byte_range = self.byte_range
    end
    if self.match_limit ~= nil then
        req.match_limit = self.match_limit
    end
    if self.max_start_depth ~= nil then
        req.max_start_depth = self.max_start_depth
    end
    local res, err = call(req)
    if err ~= nil then
        return nil, fail(err)
    end
    return res, node.tree
end

function Query:matches(node, source)
    local res, tree = query_exec(self, node, source, "matches")
    if res == nil then
        return nil, tree
    end
    self.exceeded = res.did_exceed_match_limit or false
    local out = {}
    for _, m in ipairs(res.matches or {}) do
        local captures = {}
        for _, c in ipairs(m.captures) do
            table.insert(captures, {
                node = wrap_node(tree, c.node),
                index = c.index,
                name = c.name,
            })
        end
        table.insert(out, { id = m.id, pattern = m.pattern, captures = captures })
    end
    return out
end

function Query:captures(node, source)
    local res, tree = query_exec(self, node, source, "captures")
    if res == nil then
        return nil, tree
    end
    local out = {}
    for _, c in ipairs(res.captures or {}) do
        local item = {
            node = wrap_node(tree, c.node),
            index = c.index,
            name = c.name,
        }
        if c.end_byte <= #source then
            item.text = source:sub(c.start_byte + 1, c.end_byte)
        end
        table.insert(out, item)
    end
    return out
end

function Query:pattern_count()
    return self.meta.pattern_count
end

function Query:capture_count()
    return self.meta.capture_count
end

function Query:string_count()
    return 0
end

function Query:capture_name_for_id(id)
    return self.meta.capture_names[id + 1]
end

function Query:capture_index_for_name(name)
    for idx, n in ipairs(self.meta.capture_names) do
        if n == name then
            return idx - 1
        end
    end
    return nil
end

function Query:start_byte_for_pattern(pattern)
    local p = self.meta.patterns[pattern + 1]
    return p and p.start_byte or 0
end

function Query:end_byte_for_pattern(pattern)
    local p = self.meta.patterns[pattern + 1]
    return p and p.end_byte or 0
end

function Query:is_pattern_rooted(pattern)
    local p = self.meta.patterns[pattern + 1]
    return p ~= nil and p.rooted or false
end

function Query:is_pattern_non_local(pattern)
    local p = self.meta.patterns[pattern + 1]
    return p ~= nil and p.non_local or false
end

function Query:is_pattern_guaranteed()
    return false
end

function Query:set_byte_range(start_byte, end_byte)
    self.byte_range = { start_byte, end_byte }
end

function Query:set_point_range(start_point, end_point)
    self.point_range = { start_point, end_point }
end

function Query:set_match_limit(limit)
    self.match_limit = limit
end

function Query:get_match_limit()
    return self.match_limit or 0
end

function Query:did_exceed_match_limit()
    return self.exceeded
end

function Query:set_timeout(_)
end

function Query:get_timeout()
    return 0
end

function Query:set_max_start_depth(depth)
    self.max_start_depth = depth
end

function Query:disable_pattern(pattern)
    table.insert(self.disabled_patterns, pattern)
end

function Query:disable_capture(name)
    table.insert(self.disabled_captures, name)
end

function Query:capture_quantifier()
    return nil
end

function Query:get_property_predicates()
    return {}
end

function Query:get_property_settings()
    return {}
end

function Query:get_text_predicates()
    return {}
end

function Query:close()
    self.closed = true
end

local Language = {}
Language.__index = Language

local function build_language(data: any): any
    return setmetatable({ data = data }, Language)
end

function Language:version()
    return self.data.version
end

function Language:node_kind_count()
    return self.data.node_kind_count
end

function Language:parse_state_count()
    return self.data.parse_state_count
end

function Language:field_count()
    return self.data.field_count
end

function Language:node_kind_for_id(id)
    local kind = self.data.kinds[id + 1]
    return kind and kind.name
end

function Language:id_for_node_kind(kind, named)
    for _, k in ipairs(self.data.kinds) do
        if k.name == kind and k.named == named then
            return k.id
        end
    end
    return 0
end

function Language:node_kind_is_named(id)
    local kind = self.data.kinds[id + 1]
    return kind ~= nil and kind.named or false
end

function Language:field_name_for_id(id)
    for _, f in ipairs(self.data.fields) do
        if f.id == id then
            return f.name
        end
    end
    return nil
end

function Language:field_id_for_name(name)
    for _, f in ipairs(self.data.fields) do
        if f.name == name then
            return f.id
        end
    end
    return 0
end

local Tree = {}
Tree.__index = Tree

local function build_tree(res: any, source: any?, alias: any?): any
    return setmetatable({
        handle = res.handle,
        n = res.n,
        root = res.root,
        source = source,
        lang_alias = alias,
        byte_offset = 0,
        row_offset = 0,
        column_offset = 0,
        closed = false,
    }, Tree)
end

local function check_tree(self: any)
    if self.closed then
        error("tree already closed", 2)
    end
end

function Tree:root_node()
    check_tree(self)
    return wrap_node(self, self.root)
end

function Tree:root_node_with_offset(offset_bytes, offset_point)
    check_tree(self)
    local view = setmetatable({}, Tree)
    for key, value in pairs(self) do
        view[key] = value
    end
    view.byte_offset = offset_bytes
    view.row_offset = offset_point.row
    view.column_offset = offset_point.column
    return wrap_node(view, view.root)
end

function Tree:language()
    check_tree(self)
    local data, err = call({ op = "language", language = self.lang_alias })
    if err ~= nil then
        return nil, fail(err)
    end
    return build_language(data)
end

function Tree:copy()
    check_tree(self)
    local res, err = call({ op = "clone", handle = self.handle })
    if err ~= nil then
        return nil, fail(err, errors.INTERNAL)
    end
    return build_tree({ handle = res.handle, n = self.n, root = self.root }, self.source, self.lang_alias)
end

function Tree:walk()
    check_tree(self)
    return new_cursor(self, self.root)
end

function Tree:edit(edit)
    check_tree(self)
    local function num(key: any): any
        local v = edit[key]
        if type(v) ~= "number" then
            return nil
        end
        return v
    end

    local start_byte = num("start_byte")
    if start_byte == nil then
        return false, fail("start_byte must be a number")
    end
    local old_end_byte = num("old_end_byte")
    if old_end_byte == nil then
        return false, fail("old_end_byte must be a number")
    end
    local new_end_byte = num("new_end_byte")
    if new_end_byte == nil then
        return false, fail("new_end_byte must be a number")
    end
    if start_byte < 0 or old_end_byte < start_byte or new_end_byte < 0 then
        return false, fail("invalid byte position")
    end
    for _, key in ipairs({ "start_row", "start_column", "old_end_row", "old_end_column", "new_end_row", "new_end_column" }) do
        if num(key) == nil then
            return false, fail(key .. " must be a number")
        end
    end
    if num("start_row") < 0 or num("start_column") < 0
        or num("old_end_row") < num("start_row")
        or (num("old_end_row") == num("start_row") and num("old_end_column") < num("start_column"))
        or num("new_end_row") < 0 or num("new_end_column") < 0 then
        return false, fail("invalid point position")
    end

    local res, err = call({ op = "edit", handle = self.handle, edit = edit })
    if err ~= nil then
        return false, fail(err, errors.INTERNAL)
    end
    self.n = res.n
    return true
end

local function ranges_from(res: any): any
    local out = {}
    for _, r in ipairs(res.ranges or {}) do
        table.insert(out, {
            start_byte = r.start_byte,
            end_byte = r.end_byte,
            start_point = { row = r.start_point.row, column = r.start_point.column },
            end_point = { row = r.end_point.row, column = r.end_point.column },
        })
    end
    return out
end

function Tree:changed_ranges(other)
    check_tree(self)
    if type(other) ~= "table" or other.handle == nil then
        error("Tree expected", 2)
    end
    local res, err = call({ op = "changed_ranges", handle = self.handle, other = other.handle })
    if err ~= nil then
        return {}
    end
    return ranges_from(res)
end

function Tree:included_ranges()
    check_tree(self)
    local res, err = call({ op = "included_ranges", handle = self.handle })
    if err ~= nil then
        return {}
    end
    return ranges_from(res)
end

function Tree:dot_graph()
    check_tree(self)
    return nil, fail("dot_graph is not supported by the WebAssembly engine", errors.INTERNAL)
end

function Tree:close()
    if self.closed then
        return
    end
    self.closed = true
    call({ op = "free", handle = self.handle })
end

Tree.__gc = function(self)
    if not self.closed then
        self.closed = true
        call({ op = "free", handle = self.handle })
    end
end

local Parser = {}
Parser.__index = Parser

function Parser:set_language(alias)
    local name = ALIAS_TO_NAME[alias]
    if name == nil then
        return false, fail("language " .. alias .. " is not found")
    end
    self.lang = alias
    self.name = name
    return true
end

function Parser:get_language()
    if self.name == nil then
        return nil, fail("language is not set")
    end
    return self.name
end

function Parser:parse(code, _old_tree)
    if self.lang == nil then
        return nil, fail("language is not set")
    end
    local res, err = call({ op = "parse", language = self.lang, code = code })
    if err ~= nil then
        return nil, fail(err, errors.INTERNAL)
    end
    return build_tree(res, code, self.lang)
end

function Parser:reset()
end

function Parser:set_timeout(_)
end

function Parser:set_ranges(ranges)
    for _, key in ipairs({ "start_byte", "end_byte", "start_row", "start_col", "end_row", "end_col" }) do
        for _, r in ipairs(ranges) do
            if type(r[key]) ~= "number" then
                return false, fail(key .. " must be a number")
            end
        end
    end
    self.ranges = ranges
    return true
end

function Parser:close()
    self.closed = true
end

local M = {}

function M.supported_languages(): any
    local res, err = call({ op = "languages" })
    if err ~= nil then
        return {}
    end
    return res.languages
end

function M.language(alias: any): (any, any?)
    if ALIAS_TO_NAME[alias] == nil then
        return nil, fail("unsupported language: " .. alias)
    end
    local data, err = call({ op = "language", language = alias })
    if err ~= nil then
        return nil, fail(err)
    end
    return build_language(data)
end

function M.parser(): any
    return setmetatable({ lang = nil, name = nil, ranges = nil, closed = false }, Parser)
end

function M.parse(alias: any, code: any): (any, any?)
    if ALIAS_TO_NAME[alias] == nil then
        return nil, fail("unsupported language: " .. alias)
    end
    local res, err = call({ op = "parse", language = alias, code = code })
    if err ~= nil then
        return nil, fail(err, errors.INTERNAL)
    end
    return build_tree(res, code, alias)
end

function M.query(alias: any, pattern: any): (any, any?)
    if ALIAS_TO_NAME[alias] == nil then
        return nil, fail("unsupported language: " .. alias)
    end
    local meta, err = call({ op = "query_new", language = alias, pattern = pattern })
    if err ~= nil then
        return nil, fail(err)
    end
    return build_query(alias, pattern, meta)
end

return M
