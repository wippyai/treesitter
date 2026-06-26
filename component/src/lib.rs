#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

#[cfg(target_arch = "wasm32")]
wit_bindgen::generate!({ world: "treesitter" });

#[cfg(target_arch = "wasm32")]
struct Component;

#[cfg(target_arch = "wasm32")]
impl Guest for Component {
    fn op(request: String) -> String {
        dispatch(&request)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

const MAX_LIVE_TREES: usize = 1024;

struct StoredTree {
    tree: Tree,
    lang: String,
}

struct Store {
    trees: BTreeMap<u64, StoredTree>,
    next: u64,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        Mutex::new(Store {
            trees: BTreeMap::new(),
            next: 1,
        })
    })
}

fn err(msg: impl Into<String>) -> String {
    json!({ "error": msg.into() }).to_string()
}

struct LangEntry {
    name: &'static str,
    aliases: &'static [&'static str],
    func: fn() -> Language,
}

fn lang_lua() -> Language {
    tree_sitter_lua::LANGUAGE.into()
}

fn lang_php() -> Language {
    tree_sitter_php::LANGUAGE_PHP.into()
}

fn lang_go() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

fn lang_javascript() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}

fn lang_tsx() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

fn lang_typescript() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

fn lang_python() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

fn lang_csharp() -> Language {
    tree_sitter_c_sharp::LANGUAGE.into()
}

fn lang_html() -> Language {
    tree_sitter_html::LANGUAGE.into()
}

static REGISTRY: &[LangEntry] = &[
    LangEntry { name: "lua", aliases: &["lua"], func: lang_lua },
    LangEntry { name: "php", aliases: &["php"], func: lang_php },
    LangEntry { name: "go", aliases: &["go", "golang"], func: lang_go },
    LangEntry { name: "javascript", aliases: &["js", "javascript"], func: lang_javascript },
    LangEntry { name: "typescript+jsx", aliases: &["tsx"], func: lang_tsx },
    LangEntry { name: "typescript", aliases: &["ts", "typescript"], func: lang_typescript },
    LangEntry { name: "python", aliases: &["python", "py"], func: lang_python },
    LangEntry { name: "c#", aliases: &["csharp", "c#", "cs"], func: lang_csharp },
    LangEntry { name: "html", aliases: &["html", "html5"], func: lang_html },
];

fn language_for(alias: &str) -> Option<Language> {
    for entry in REGISTRY {
        if entry.aliases.contains(&alias) {
            return Some((entry.func)());
        }
    }
    None
}

#[derive(Serialize)]
struct Point {
    row: usize,
    column: usize,
}

#[derive(Serialize)]
struct NodeJson {
    id: usize,
    kind: String,
    grammar_name: String,
    is_named: bool,
    is_extra: bool,
    is_missing: bool,
    is_error: bool,
    has_error: bool,
    start_byte: usize,
    end_byte: usize,
    start_point: Point,
    end_point: Point,
    parent: i64,
    children: Vec<usize>,
    named_children: Vec<usize>,
    fields: BTreeMap<usize, String>,
    field_ids: BTreeMap<usize, u16>,
    descendant_count: usize,
}

fn serialize_node(node: Node, parent: i64, lang: &Language, out: &mut Vec<NodeJson>) -> usize {
    let idx = out.len();
    let sp = node.start_position();
    let ep = node.end_position();
    out.push(NodeJson {
        id: idx,
        kind: node.kind().to_string(),
        grammar_name: node.grammar_name().to_string(),
        is_named: node.is_named(),
        is_extra: node.is_extra(),
        is_missing: node.is_missing(),
        is_error: node.is_error(),
        has_error: node.has_error(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_point: Point { row: sp.row, column: sp.column },
        end_point: Point { row: ep.row, column: ep.column },
        parent,
        children: Vec::new(),
        named_children: Vec::new(),
        fields: BTreeMap::new(),
        field_ids: BTreeMap::new(),
        descendant_count: node.descendant_count(),
    });

    let mut children = Vec::new();
    let mut named_children = Vec::new();
    let mut fields = BTreeMap::new();
    let mut field_ids = BTreeMap::new();
    let count = node.child_count();
    let mut pos = 0usize;
    while pos < count {
        if let Some(child) = node.child(pos) {
            let cidx = serialize_node(child, idx as i64, lang, out);
            if let Some(name) = node.field_name_for_child(pos as u32) {
                fields.insert(pos, name.to_string());
                if let Some(fid) = lang.field_id_for_name(name) {
                    field_ids.insert(pos, fid.get());
                }
            }
            if child.is_named() {
                named_children.push(cidx);
            }
            children.push(cidx);
        }
        pos += 1;
    }

    let n = &mut out[idx];
    n.children = children;
    n.named_children = named_children;
    n.fields = fields;
    n.field_ids = field_ids;
    idx
}

fn serialize_tree(tree: &Tree, lang: &Language) -> Vec<NodeJson> {
    let mut out = Vec::new();
    serialize_node(tree.root_node(), -1, lang, &mut out);
    out
}

fn node_at_index(root: Node, target: usize) -> Option<Node> {
    fn walk<'a>(node: Node<'a>, counter: &mut usize, target: usize) -> Option<Node<'a>> {
        if *counter == target {
            return Some(node);
        }
        *counter += 1;
        let count = node.child_count();
        let mut i = 0usize;
        while i < count {
            if let Some(child) = node.child(i) {
                if let Some(found) = walk(child, counter, target) {
                    return Some(found);
                }
            }
            i += 1;
        }
        None
    }
    let mut counter = 0usize;
    walk(root, &mut counter, target)
}

fn build_id_index(root: Node) -> BTreeMap<usize, usize> {
    fn walk(node: Node, counter: &mut usize, map: &mut BTreeMap<usize, usize>) {
        map.insert(node.id(), *counter);
        *counter += 1;
        let count = node.child_count();
        let mut i = 0usize;
        while i < count {
            if let Some(child) = node.child(i) {
                walk(child, counter, map);
            }
            i += 1;
        }
    }
    let mut map = BTreeMap::new();
    let mut counter = 0usize;
    walk(root, &mut counter, &mut map);
    map
}

fn op_parse(req: &Value) -> String {
    let language = req.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let code = match req.get("code").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return err("parse: 'code' required"),
    };
    let lang = match language_for(language) {
        Some(l) => l,
        None => return err(format!("unsupported language: {}", language)),
    };
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return err("failed to set language");
    }
    let tree = match parser.parse(code, None) {
        Some(t) => t,
        None => return err("failed to parse code"),
    };

    let nodes = serialize_tree(&tree, &lang);

    let mut s = store().lock().unwrap();
    if s.trees.len() >= MAX_LIVE_TREES {
        return err("too many open trees; close some before parsing more");
    }
    let handle = s.next;
    s.next += 1;
    s.trees.insert(handle, StoredTree { tree, lang: language.to_string() });

    json!({ "handle": handle, "root": 0, "nodes": nodes }).to_string()
}

fn op_clone(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("clone: 'handle' required"),
    };
    let mut s = store().lock().unwrap();
    let cloned = match s.trees.get(&handle) {
        Some(st) => StoredTree { tree: st.tree.clone(), lang: st.lang.clone() },
        None => return err("clone: unknown handle"),
    };
    if s.trees.len() >= MAX_LIVE_TREES {
        return err("too many open trees; close some before cloning more");
    }
    let new_handle = s.next;
    s.next += 1;
    s.trees.insert(new_handle, cloned);
    json!({ "handle": new_handle }).to_string()
}

fn point_json(p: tree_sitter::Point) -> Value {
    json!({ "row": p.row, "column": p.column })
}

fn range_json(r: &tree_sitter::Range) -> Value {
    json!({
        "start_byte": r.start_byte,
        "end_byte": r.end_byte,
        "start_point": point_json(r.start_point),
        "end_point": point_json(r.end_point),
    })
}

fn read_point(v: &Value, row_key: &str, col_key: &str) -> tree_sitter::Point {
    tree_sitter::Point {
        row: v.get(row_key).and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        column: v.get(col_key).and_then(|x| x.as_u64()).unwrap_or(0) as usize,
    }
}

fn op_edit(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("edit: 'handle' required"),
    };
    let e = match req.get("edit") {
        Some(e) => e,
        None => return err("edit: 'edit' required"),
    };
    let input = tree_sitter::InputEdit {
        start_byte: e.get("start_byte").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        old_end_byte: e.get("old_end_byte").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        new_end_byte: e.get("new_end_byte").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        start_position: read_point(e, "start_row", "start_column"),
        old_end_position: read_point(e, "old_end_row", "old_end_column"),
        new_end_position: read_point(e, "new_end_row", "new_end_column"),
    };
    let mut s = store().lock().unwrap();
    let stored = match s.trees.get_mut(&handle) {
        Some(st) => st,
        None => return err("edit: unknown handle"),
    };
    let lang = match language_for(&stored.lang) {
        Some(l) => l,
        None => return err("edit: stored language unavailable"),
    };
    stored.tree.edit(&input);
    let nodes = serialize_tree(&stored.tree, &lang);
    json!({ "handle": handle, "root": 0, "nodes": nodes }).to_string()
}

fn op_changed_ranges(req: &Value) -> String {
    let handle = req.get("handle").and_then(|v| v.as_u64());
    let other = req.get("other").and_then(|v| v.as_u64());
    let (handle, other) = match (handle, other) {
        (Some(h), Some(o)) => (h, o),
        _ => return err("changed_ranges: 'handle' and 'other' required"),
    };
    let s = store().lock().unwrap();
    let a = match s.trees.get(&handle) {
        Some(st) => st,
        None => return err("changed_ranges: unknown handle"),
    };
    let b = match s.trees.get(&other) {
        Some(st) => st,
        None => return err("changed_ranges: unknown other handle"),
    };
    let ranges: Vec<Value> = a.tree.changed_ranges(&b.tree).map(|r| range_json(&r)).collect();
    json!({ "ranges": ranges }).to_string()
}

fn op_sexp(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("sexp: 'handle' required"),
    };
    let node_index = req.get("node").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let s = store().lock().unwrap();
    let st = match s.trees.get(&handle) {
        Some(st) => st,
        None => return err("sexp: unknown handle"),
    };
    let node = match node_at_index(st.tree.root_node(), node_index) {
        Some(n) => n,
        None => return err("sexp: node index out of range"),
    };
    json!({ "sexp": node.to_sexp() }).to_string()
}

fn op_included_ranges(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("included_ranges: 'handle' required"),
    };
    let s = store().lock().unwrap();
    let st = match s.trees.get(&handle) {
        Some(st) => st,
        None => return err("included_ranges: unknown handle"),
    };
    let ranges: Vec<Value> = st.tree.included_ranges().iter().map(range_json).collect();
    json!({ "ranges": ranges }).to_string()
}

fn op_free(req: &Value) -> String {
    if let Some(h) = req.get("handle").and_then(|v| v.as_u64()) {
        store().lock().unwrap().trees.remove(&h);
    }
    json!({}).to_string()
}

fn op_languages() -> String {
    let mut languages = serde_json::Map::new();
    for entry in REGISTRY {
        languages.insert(entry.name.to_string(), Value::Bool(true));
    }
    json!({ "languages": languages }).to_string()
}

fn op_language(req: &Value) -> String {
    let language = req.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let lang = match language_for(language) {
        Some(l) => l,
        None => return err(format!("unsupported language: {}", language)),
    };

    let node_kind_count = lang.node_kind_count();
    let mut kinds = Vec::with_capacity(node_kind_count);
    let mut id = 0u16;
    while (id as usize) < node_kind_count {
        kinds.push(json!({
            "id": id,
            "name": lang.node_kind_for_id(id).unwrap_or(""),
            "named": lang.node_kind_is_named(id),
        }));
        id += 1;
    }

    let field_count = lang.field_count();
    let mut fields = Vec::with_capacity(field_count);
    let mut fid = 1u16;
    while (fid as usize) <= field_count {
        if let Some(name) = lang.field_name_for_id(fid) {
            fields.push(json!({ "id": fid, "name": name }));
        }
        fid += 1;
    }

    json!({
        "version": lang.abi_version(),
        "node_kind_count": node_kind_count,
        "parse_state_count": lang.parse_state_count(),
        "field_count": field_count,
        "kinds": kinds,
        "fields": fields,
    })
    .to_string()
}

fn op_query_new(req: &Value) -> String {
    let language = req.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let pattern = match req.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return err("query: 'pattern' required"),
    };
    let lang = match language_for(language) {
        Some(l) => l,
        None => return err(format!("unsupported language: {}", language)),
    };
    let query = match Query::new(&lang, pattern) {
        Ok(q) => q,
        Err(e) => return err(format!("{} {}", e, e.message)),
    };

    let capture_names: Vec<&str> = query.capture_names().to_vec();
    let pattern_count = query.pattern_count();
    let mut patterns = Vec::with_capacity(pattern_count);
    let mut i = 0usize;
    while i < pattern_count {
        patterns.push(json!({
            "rooted": query.is_pattern_rooted(i),
            "non_local": query.is_pattern_non_local(i),
            "start_byte": query.start_byte_for_pattern(i),
        }));
        i += 1;
    }

    json!({
        "pattern_count": pattern_count,
        "capture_count": capture_names.len(),
        "capture_names": capture_names,
        "patterns": patterns,
    })
    .to_string()
}

fn op_query(req: &Value) -> String {
    let language = req.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let pattern = match req.get("pattern").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return err("query: 'pattern' required"),
    };
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("query: 'handle' required"),
    };
    let node_index = req.get("node").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let source = req.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let mode = req.get("mode").and_then(|v| v.as_str()).unwrap_or("captures");

    let lang = match language_for(language) {
        Some(l) => l,
        None => return err(format!("unsupported language: {}", language)),
    };
    let mut query = match Query::new(&lang, pattern) {
        Ok(q) => q,
        Err(e) => return err(format!("{} {}", e, e.message)),
    };

    if let Some(list) = req.get("disable_patterns").and_then(|v| v.as_array()) {
        for p in list {
            if let Some(p) = p.as_u64() {
                query.disable_pattern(p as usize);
            }
        }
    }
    if let Some(list) = req.get("disable_captures").and_then(|v| v.as_array()) {
        for name in list {
            if let Some(name) = name.as_str() {
                query.disable_capture(name);
            }
        }
    }

    let s = store().lock().unwrap();
    let tree = match s.trees.get(&handle) {
        Some(st) => &st.tree,
        None => return err("query: unknown handle"),
    };
    let root = tree.root_node();
    let node = match node_at_index(root, node_index) {
        Some(n) => n,
        None => return err("query: node index out of range"),
    };
    let id_index = build_id_index(root);

    let mut cursor = QueryCursor::new();
    if let Some(limit) = req.get("match_limit").and_then(|v| v.as_u64()) {
        cursor.set_match_limit(limit as u32);
    }
    if let Some(depth) = req.get("max_start_depth").and_then(|v| v.as_u64()) {
        cursor.set_max_start_depth(Some(depth as u32));
    }
    if let Some(range) = req.get("byte_range").and_then(|v| v.as_array()) {
        if range.len() == 2 {
            let start = range[0].as_u64().unwrap_or(0) as usize;
            let end = range[1].as_u64().unwrap_or(0) as usize;
            cursor.set_byte_range(start..end);
        }
    }

    let names = query.capture_names();
    let src = source.as_bytes();

    if mode == "matches" {
        let mut it = cursor.matches(&query, node, src);
        let mut matches = Vec::new();
        while let Some(m) = it.next() {
            let captures: Vec<Value> = m
                .captures
                .iter()
                .map(|c| {
                    json!({
                        "node": id_index.get(&c.node.id()),
                        "index": c.index,
                        "name": names.get(c.index as usize).copied().unwrap_or(""),
                    })
                })
                .collect();
            matches.push(json!({
                "id": m.id(),
                "pattern": m.pattern_index,
                "captures": captures,
            }));
        }
        let exceeded = cursor.did_exceed_match_limit();
        json!({ "matches": matches, "did_exceed_match_limit": exceeded }).to_string()
    } else {
        let mut it = cursor.captures(&query, node, src);
        let mut captures = Vec::new();
        while let Some((m, capture_index)) = it.next() {
            let c = &m.captures[*capture_index];
            captures.push(json!({
                "node": id_index.get(&c.node.id()),
                "index": c.index,
                "name": names.get(c.index as usize).copied().unwrap_or(""),
                "start_byte": c.node.start_byte(),
                "end_byte": c.node.end_byte(),
            }));
        }
        json!({ "captures": captures }).to_string()
    }
}

fn dispatch(request: &str) -> String {
    let req: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return err(format!("invalid request json: {}", e)),
    };
    match req.get("op").and_then(|v| v.as_str()).unwrap_or("") {
        "parse" => op_parse(&req),
        "clone" => op_clone(&req),
        "edit" => op_edit(&req),
        "changed_ranges" => op_changed_ranges(&req),
        "included_ranges" => op_included_ranges(&req),
        "sexp" => op_sexp(&req),
        "free" => op_free(&req),
        "languages" => op_languages(),
        "language" => op_language(&req),
        "query_new" => op_query_new(&req),
        "query" => op_query(&req),
        "" => err("missing 'op'"),
        other => err(format!("unknown op: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(language: &str, code: &str) -> Value {
        let out = dispatch(&json!({ "op": "parse", "language": language, "code": code }).to_string());
        serde_json::from_str(&out).unwrap()
    }

    #[test]
    fn parse_go_serializes_tree() {
        let v = parse("go", "package main\n\nfunc hello() string { return \"hi\" }\n");
        assert!(v.get("error").is_none(), "unexpected error: {}", v);
        assert_eq!(v["root"], json!(0));
        let nodes = v["nodes"].as_array().unwrap();
        assert_eq!(nodes[0]["kind"], json!("source_file"));
        assert_eq!(nodes[0]["parent"], json!(-1));
    }

    #[test]
    fn query_captures_function_names() {
        let v = parse("go", "package main\nfunc hello() {}\nfunc world() {}\n");
        let handle = v["handle"].as_u64().unwrap();
        let code = "package main\nfunc hello() {}\nfunc world() {}\n";
        let out = dispatch(
            &json!({
                "op": "query",
                "language": "go",
                "pattern": "(function_declaration name: (identifier) @name)",
                "handle": handle,
                "node": 0,
                "source": code,
                "mode": "captures",
            })
            .to_string(),
        );
        let r: Value = serde_json::from_str(&out).unwrap();
        let caps = r["captures"].as_array().unwrap();
        assert_eq!(caps.len(), 2);
        assert_eq!(caps[0]["name"], json!("name"));
        let nodes = v["nodes"].as_array().unwrap();
        let first = caps[0]["node"].as_u64().unwrap() as usize;
        let slice = &code[nodes[first]["start_byte"].as_u64().unwrap() as usize
            ..nodes[first]["end_byte"].as_u64().unwrap() as usize];
        assert_eq!(slice, "hello");
    }

    #[test]
    fn language_introspection() {
        let out = dispatch(&json!({ "op": "language", "language": "go" }).to_string());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["version"].as_u64().unwrap() > 0);
        assert!(v["node_kind_count"].as_u64().unwrap() > 0);
        assert!(v["field_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn unsupported_language_errors() {
        let out = dispatch(&json!({ "op": "parse", "language": "cobol", "code": "x" }).to_string());
        assert!(out.contains("unsupported language"));
    }

    #[test]
    fn bad_query_pattern_errors() {
        let out = dispatch(
            &json!({ "op": "query_new", "language": "go", "pattern": "(((" }).to_string(),
        );
        assert!(out.contains("error"));
    }
}
