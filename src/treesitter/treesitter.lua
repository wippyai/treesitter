local funcs = require("funcs")
local json = require("json")

local ENGINE = "treesitter:engine"

local function call(req)
    local body = json.encode(req)
    local out, err = funcs.call(ENGINE, body)
    if err ~= nil then
        return nil, tostring(err)
    end
    local decoded = json.decode(out)
    if type(decoded) == "table" and decoded.error ~= nil then
        return nil, decoded.error
    end
    return decoded, nil
end

local Node = {}
Node.__index = Node

local function wrap_node(tree, id)
    if id == nil or id < 0 then
        return nil
    end
    return setmetatable({ tree = tree, i = id }, Node)
end

local function raw(self)
    return self.tree.nodes[self.i + 1]
end

function Node:kind()
    return raw(self).kind
end

Node.type = Node.kind

function Node:grammar_name()
    return raw(self).grammar_name
end

function Node:is_named()
    return raw(self).is_named
end

function Node:is_extra()
    return raw(self).is_extra
end

function Node:is_missing()
    return raw(self).is_missing
end

function Node:is_error()
    return raw(self).is_error
end

function Node:has_error()
    return raw(self).has_error
end

function Node:start_byte()
    return raw(self).start_byte
end

function Node:end_byte()
    return raw(self).end_byte
end

function Node:start_point()
    local p = raw(self).start_point
    return { row = p.row, column = p.column }
end

function Node:end_point()
    local p = raw(self).end_point
    return { row = p.row, column = p.column }
end

function Node:child_count()
    return #raw(self).children
end

function Node:named_child_count()
    return #raw(self).named_children
end

function Node:child(index)
    return wrap_node(self.tree, raw(self).children[index + 1])
end

function Node:named_child(index)
    return wrap_node(self.tree, raw(self).named_children[index + 1])
end

function Node:parent()
    local p = raw(self).parent
    if p == nil or p < 0 then
        return nil
    end
    return wrap_node(self.tree, p)
end

function Node:child_by_field_name(name)
    local node = raw(self)
    for pos, field in pairs(node.fields or {}) do
        if field == name then
            return wrap_node(self.tree, node.children[tonumber(pos) + 1])
        end
    end
    return nil
end

function Node:text(source)
    local src = source or self.tree.source
    local node = raw(self)
    return src:sub(node.start_byte + 1, node.end_byte)
end

local Tree = {}
Tree.__index = Tree

function Tree:root_node()
    return wrap_node(self, self.root)
end

function Tree:close()
    if self.closed then
        return
    end
    self.closed = true
    call({ op = "free", handle = self.handle })
end

local M = {}

function M.parse(language, code)
    local res, err = call({ op = "parse", language = language, code = code })
    if err ~= nil then
        return nil, err
    end
    return setmetatable({
        handle = res.handle,
        nodes = res.nodes,
        root = res.root,
        source = code,
        language = language,
        closed = false,
    }, Tree)
end

function M.supported_languages()
    local res, err = call({ op = "languages" })
    if err ~= nil then
        return nil, err
    end
    return res.languages
end

return M
