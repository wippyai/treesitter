#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};
use tree_sitter::{Node, Parser, Tree};

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

struct Store {
    trees: BTreeMap<u64, Tree>,
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

fn language_for(alias: &str) -> Option<tree_sitter::Language> {
    let lang = match alias {
        "go" | "golang" => tree_sitter_go::LANGUAGE,
        _ => return None,
    };
    Some(lang.into())
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
    descendant_count: usize,
}

fn serialize_node(node: Node, parent: i64, out: &mut Vec<NodeJson>) -> usize {
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
        start_point: Point {
            row: sp.row,
            column: sp.column,
        },
        end_point: Point {
            row: ep.row,
            column: ep.column,
        },
        parent,
        children: Vec::new(),
        named_children: Vec::new(),
        fields: BTreeMap::new(),
        descendant_count: node.descendant_count(),
    });

    let mut children = Vec::new();
    let mut named_children = Vec::new();
    let mut fields = BTreeMap::new();
    let mut cursor = node.walk();
    let mut pos = 0usize;
    for child in node.children(&mut cursor) {
        let cidx = serialize_node(child, idx as i64, out);
        if let Some(name) = node.field_name_for_child(pos as u32) {
            fields.insert(pos, name.to_string());
        }
        if child.is_named() {
            named_children.push(cidx);
        }
        children.push(cidx);
        pos += 1;
    }

    let n = &mut out[idx];
    n.children = children;
    n.named_children = named_children;
    n.fields = fields;
    idx
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

    let mut nodes = Vec::new();
    serialize_node(tree.root_node(), -1, &mut nodes);

    let mut s = store().lock().unwrap();
    if s.trees.len() >= MAX_LIVE_TREES {
        return err("too many open trees; close some before parsing more");
    }
    let handle = s.next;
    s.next += 1;
    s.trees.insert(handle, tree);

    json!({ "handle": handle, "root": 0, "nodes": nodes }).to_string()
}

fn op_free(req: &Value) -> String {
    if let Some(h) = req.get("handle").and_then(|v| v.as_u64()) {
        store().lock().unwrap().trees.remove(&h);
    }
    json!({}).to_string()
}

fn op_languages() -> String {
    json!({ "languages": { "go": true } }).to_string()
}

fn dispatch(request: &str) -> String {
    let req: Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(e) => return err(format!("invalid request json: {}", e)),
    };
    match req.get("op").and_then(|v| v.as_str()).unwrap_or("") {
        "parse" => op_parse(&req),
        "free" => op_free(&req),
        "languages" => op_languages(),
        "" => err("missing 'op'"),
        other => err(format!("unknown op: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_go_serializes_tree() {
        let code = "package main\n\nfunc hello() string { return \"hi\" }\n";
        let out = dispatch(&json!({ "op": "parse", "language": "go", "code": code }).to_string());
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("error").is_none(), "unexpected error: {}", out);
        assert_eq!(v["root"], json!(0));
        let nodes = v["nodes"].as_array().unwrap();
        assert!(!nodes.is_empty());
        assert_eq!(nodes[0]["kind"], json!("source_file"));
        assert_eq!(nodes[0]["parent"], json!(-1));
        assert!(nodes[0]["children"].as_array().unwrap().len() >= 1);
    }

    #[test]
    fn unsupported_language_errors() {
        let out = dispatch(&json!({ "op": "parse", "language": "cobol", "code": "x" }).to_string());
        assert!(out.contains("unsupported language"));
    }
}
