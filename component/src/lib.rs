#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

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
    cols: Option<Columns>,
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

#[derive(Clone)]
struct Columns {
    n: usize,
    root: usize,
    kinds: Vec<String>,
    k: Vec<usize>,
    gn: BTreeMap<usize, usize>,
    sb: Vec<usize>,
    eb: Vec<usize>,
    sr: Vec<usize>,
    sc: Vec<usize>,
    er: Vec<usize>,
    ec: Vec<usize>,
    parent: Vec<i64>,
    flags: Vec<u8>,
    dc: Vec<usize>,
    ch_off: Vec<usize>,
    ch_val: Vec<usize>,
    fields: BTreeMap<usize, BTreeMap<usize, String>>,
    field_ids: BTreeMap<usize, BTreeMap<usize, u16>>,
}

#[derive(Default)]
struct Builder {
    kinds: Vec<String>,
    kind_index: BTreeMap<String, usize>,
    k: Vec<usize>,
    gn: BTreeMap<usize, usize>,
    sb: Vec<usize>,
    eb: Vec<usize>,
    sr: Vec<usize>,
    sc: Vec<usize>,
    er: Vec<usize>,
    ec: Vec<usize>,
    parent: Vec<i64>,
    flags: Vec<u8>,
    dc: Vec<usize>,
    children: Vec<Vec<usize>>,
    fields: BTreeMap<usize, BTreeMap<usize, String>>,
    field_ids: BTreeMap<usize, BTreeMap<usize, u16>>,
}

impl Builder {
    fn intern(&mut self, s: &str) -> usize {
        if let Some(&id) = self.kind_index.get(s) {
            return id;
        }
        let id = self.kinds.len();
        self.kinds.push(s.to_string());
        self.kind_index.insert(s.to_string(), id);
        id
    }

    fn visit(&mut self, node: Node, parent: i64, lang: &Language) -> usize {
        let idx = self.k.len();
        let kind = node.kind();
        let kid = self.intern(kind);
        self.k.push(kid);
        let grammar = node.grammar_name();
        if grammar != kind {
            let gid = self.intern(grammar);
            self.gn.insert(idx, gid);
        }
        let sp = node.start_position();
        let ep = node.end_position();
        self.sb.push(node.start_byte());
        self.eb.push(node.end_byte());
        self.sr.push(sp.row);
        self.sc.push(sp.column);
        self.er.push(ep.row);
        self.ec.push(ep.column);
        self.parent.push(parent);
        let mut flag = 0u8;
        if node.is_named() {
            flag |= 1;
        }
        if node.is_extra() {
            flag |= 2;
        }
        if node.is_missing() {
            flag |= 4;
        }
        if node.is_error() {
            flag |= 8;
        }
        if node.has_error() {
            flag |= 16;
        }
        self.flags.push(flag);
        self.dc.push(node.descendant_count());
        self.children.push(Vec::new());

        let mut kids = Vec::new();
        let mut fmap = BTreeMap::new();
        let mut fidmap = BTreeMap::new();
        let count = node.child_count();
        let mut pos = 0usize;
        while pos < count {
            if let Some(child) = node.child(pos) {
                let child_pos = kids.len();
                let cidx = self.visit(child, idx as i64, lang);
                if let Some(name) = node.field_name_for_child(pos as u32) {
                    fmap.insert(child_pos, name.to_string());
                    if let Some(fid) = lang.field_id_for_name(name) {
                        fidmap.insert(child_pos, fid.get());
                    }
                }
                kids.push(cidx);
            }
            pos += 1;
        }
        self.children[idx] = kids;
        if !fmap.is_empty() {
            self.fields.insert(idx, fmap);
        }
        if !fidmap.is_empty() {
            self.field_ids.insert(idx, fidmap);
        }
        idx
    }

    fn finish(self) -> Columns {
        let n = self.k.len();
        let mut ch_off = Vec::with_capacity(n + 1);
        let mut ch_val = Vec::new();
        let mut acc = 0usize;
        ch_off.push(0);
        for kids in &self.children {
            acc += kids.len();
            for &c in kids {
                ch_val.push(c);
            }
            ch_off.push(acc);
        }
        Columns {
            n,
            root: 0,
            kinds: self.kinds,
            k: self.k,
            gn: self.gn,
            sb: self.sb,
            eb: self.eb,
            sr: self.sr,
            sc: self.sc,
            er: self.er,
            ec: self.ec,
            parent: self.parent,
            flags: self.flags,
            dc: self.dc,
            ch_off,
            ch_val,
            fields: self.fields,
            field_ids: self.field_ids,
        }
    }
}

fn build_columns(tree: &Tree, lang: &Language) -> Columns {
    let mut b = Builder::default();
    b.visit(tree.root_node(), -1, lang);
    b.finish()
}

fn node_record(c: &Columns, id: usize) -> Value {
    let off = c.ch_off[id];
    let end = c.ch_off[id + 1];
    let children: Vec<usize> = c.ch_val[off..end].to_vec();
    let named_children: Vec<usize> =
        children.iter().copied().filter(|&cid| c.flags[cid] & 1 != 0).collect();
    let mut rec = json!({
        "kind": c.kinds[c.k[id]],
        "flags": c.flags[id],
        "sb": c.sb[id],
        "eb": c.eb[id],
        "sr": c.sr[id],
        "sc": c.sc[id],
        "er": c.er[id],
        "ec": c.ec[id],
        "parent": c.parent[id],
        "dc": c.dc[id],
        "children": children,
        "named_children": named_children,
    });
    if let Some(gid) = c.gn.get(&id) {
        rec["gn"] = json!(c.kinds[*gid]);
    }
    if let Some(f) = c.fields.get(&id) {
        rec["fields"] = json!(f);
    }
    if let Some(f) = c.field_ids.get(&id) {
        rec["field_ids"] = json!(f);
    }
    rec
}

fn op_node(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("node: 'handle' required"),
    };
    let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let mut s = store().lock().unwrap();
    let st = match s.trees.get_mut(&handle) {
        Some(st) => st,
        None => return err("node: unknown handle"),
    };
    if st.cols.is_none() {
        let lang = match language_for(&st.lang) {
            Some(l) => l,
            None => return err("node: stored language unavailable"),
        };
        let cols = build_columns(&st.tree, &lang);
        st.cols = Some(cols);
    }
    let c = st.cols.as_ref().unwrap();
    if id >= c.n {
        return err("node: index out of range");
    }
    node_record(c, id).to_string()
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

    let n = tree.root_node().descendant_count();

    let mut s = store().lock().unwrap();
    if s.trees.len() >= MAX_LIVE_TREES {
        return err("too many open trees; close some before parsing more");
    }
    let handle = s.next;
    s.next += 1;
    s.trees.insert(handle, StoredTree { tree, lang: language.to_string(), cols: None });

    json!({ "handle": handle, "root": 0, "n": n }).to_string()
}

fn op_clone(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("clone: 'handle' required"),
    };
    let mut s = store().lock().unwrap();
    let cloned = match s.trees.get(&handle) {
        Some(st) => StoredTree {
            tree: st.tree.clone(),
            lang: st.lang.clone(),
            cols: None,
        },
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
    stored.tree.edit(&input);
    stored.cols = None;
    let n = stored.tree.root_node().descendant_count();
    json!({ "handle": handle, "n": n }).to_string()
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

fn op_descendant(req: &Value) -> String {
    let handle = match req.get("handle").and_then(|v| v.as_u64()) {
        Some(h) => h,
        None => return err("descendant: 'handle' required"),
    };
    let id = req.get("node").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let sp = match req.get("start") {
        Some(p) => read_point(p, "row", "column"),
        None => return err("descendant: 'start' required"),
    };
    let ep = match req.get("end") {
        Some(p) => read_point(p, "row", "column"),
        None => return err("descendant: 'end' required"),
    };
    let s = store().lock().unwrap();
    let st = match s.trees.get(&handle) {
        Some(st) => st,
        None => return err("descendant: unknown handle"),
    };
    let root = st.tree.root_node();
    let node = match node_at_index(root, id) {
        Some(n) => n,
        None => return err("descendant: index out of range"),
    };
    match node.named_descendant_for_point_range(sp, ep) {
        Some(d) => {
            let map = build_id_index(root);
            json!({ "node": map.get(&d.id()) }).to_string()
        }
        None => json!({ "node": Value::Null }).to_string(),
    }
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
        "node" => op_node(&req),
        "clone" => op_clone(&req),
        "edit" => op_edit(&req),
        "changed_ranges" => op_changed_ranges(&req),
        "included_ranges" => op_included_ranges(&req),
        "descendant" => op_descendant(&req),
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
        assert!(v["n"].as_u64().unwrap() > 1);
        let handle = v["handle"].as_u64().unwrap();
        let r: Value = serde_json::from_str(
            &dispatch(&json!({ "op": "node", "handle": handle, "id": 0 }).to_string()),
        )
        .unwrap();
        assert_eq!(r["kind"], json!("source_file"));
        assert_eq!(r["parent"], json!(-1));
        assert!(r["children"].as_array().unwrap().len() >= 1);
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
        let sb = caps[0]["start_byte"].as_u64().unwrap() as usize;
        let eb = caps[0]["end_byte"].as_u64().unwrap() as usize;
        assert_eq!(&code[sb..eb], "hello");
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
