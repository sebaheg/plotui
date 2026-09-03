//! A subset of Graphviz's DOT language, and the composer that turns it into
//! a laid-out plot.
//!
//! This is not a Graphviz port and never will be. It is the smallest grammar
//! that lets someone paste a pipeline they already have — a `dot` file from
//! Airflow, `cargo tree`, a Makefile dump — and see it, plus the attributes
//! that carry meaning rather than typography: what a node is called, what
//! colour it is, what shape it is, and which way the graph flows.
//!
//! Everything else is rejected with a message that says where and why, and
//! the messages live here rather than in each binding, so Python, Go, C and
//! JavaScript report the same bytes for the same file. Unknown *attributes*
//! are ignored rather than rejected, because a real DOT file is full of
//! fonts and margins that have no meaning in a terminal and refusing to draw
//! it over `fontsize=10` would be useless.

use plotui_core::{LayeredLayout, NodeShape, Plot, RankDir, Rgb, TraceId, COLORWAY_PLOTUI};

use crate::{parse_color, BindError};

/// One declared node. `id` is the DOT identifier (the caller's handle on it);
/// `label` is what gets drawn, defaulting to the id.
#[derive(Clone, Debug, PartialEq)]
pub struct DotNode {
    pub id: String,
    pub label: String,
    pub color: Option<Rgb>,
    pub shape: NodeShape,
}

/// One edge, by node index into [`DotGraph::nodes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DotEdge {
    pub from: u32,
    pub to: u32,
    pub color: Option<Rgb>,
}

/// A parsed DOT document. Node order is declaration order, which is what
/// makes the indices stable enough to hand to a layout and back.
#[derive(Clone, Debug, PartialEq)]
pub struct DotGraph {
    pub name: Option<String>,
    pub directed: bool,
    pub rankdir: RankDir,
    pub nodes: Vec<DotNode>,
    pub edges: Vec<DotEdge>,
}

// --- lexer ---

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// An identifier: bare word, numeral, or double-quoted string. `quoted`
    /// is kept because a quoted `"node"` is a name, not the keyword.
    Id {
        text: String,
        quoted: bool,
    },
    Punct(char),
    /// `->`
    Arrow,
    /// `--`
    Line,
}

#[derive(Clone, Debug)]
struct Lexed {
    tok: Tok,
    line: usize,
    col: usize,
}

/// An error at a source position. Every message this module produces goes
/// through here, so they all carry `line:col` and all read the same way.
fn at(line: usize, col: usize, msg: impl AsRef<str>) -> BindError {
    BindError::invalid(format!("{line}:{col}: {}", msg.as_ref()))
}

fn is_id_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || (c as u32) >= 0x80
}

fn is_id_char(c: char) -> bool {
    is_id_start(c) || c.is_ascii_digit()
}

struct Lexer {
    src: Vec<char>,
    i: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    fn new(text: &str) -> Self {
        Lexer { src: text.chars().collect(), i: 0, line: 1, col: 1 }
    }

    fn peek(&self, k: usize) -> Option<char> {
        self.src.get(self.i + k).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// Whitespace and all three comment forms DOT accepts: `//` to end of
    /// line, `/* */`, and `#` (which real DOT treats as a preprocessor line).
    fn skip_trivia(&mut self) -> Result<(), BindError> {
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(c), _) if c.is_whitespace() => {
                    self.bump();
                }
                (Some('#'), _) | (Some('/'), Some('/')) => {
                    while let Some(c) = self.peek(0) {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                (Some('/'), Some('*')) => {
                    let (line, col) = (self.line, self.col);
                    self.bump();
                    self.bump();
                    loop {
                        match (self.peek(0), self.peek(1)) {
                            (Some('*'), Some('/')) => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            (Some(_), _) => {
                                self.bump();
                            }
                            (None, _) => return Err(at(line, col, "unterminated /* comment")),
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn tokens(mut self) -> Result<Vec<Lexed>, BindError> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia()?;
            let (line, col) = (self.line, self.col);
            let Some(c) = self.peek(0) else { return Ok(out) };
            let tok = match c {
                '{' | '}' | '[' | ']' | ';' | ',' | '=' => {
                    self.bump();
                    Tok::Punct(c)
                }
                // A port would change what a node *is* — an anchor on one of
                // its sides — and there are no ports to anchor to yet, so
                // silently dropping it would move the edge somewhere the
                // author did not ask for.
                ':' => {
                    return Err(at(
                        line,
                        col,
                        "node ports (a:port) are not supported; drop the ':port'",
                    ))
                }
                '<' => {
                    return Err(at(
                        line,
                        col,
                        "HTML labels (<...>) are not supported; use a quoted string",
                    ))
                }
                '"' => {
                    self.bump();
                    let mut s = String::new();
                    loop {
                        match self.bump() {
                            Some('"') => break,
                            // Only the two escapes that matter for a label;
                            // anything else keeps its backslash, so a Windows
                            // path in a label survives the round trip.
                            Some('\\') => match self.bump() {
                                Some('"') => s.push('"'),
                                Some('\\') => s.push('\\'),
                                Some('\n') => {}
                                Some(other) => {
                                    s.push('\\');
                                    s.push(other);
                                }
                                None => return Err(at(line, col, "unterminated string")),
                            },
                            Some(other) => s.push(other),
                            None => return Err(at(line, col, "unterminated string")),
                        }
                    }
                    Tok::Id { text: s, quoted: true }
                }
                '-' if self.peek(1) == Some('>') => {
                    self.bump();
                    self.bump();
                    Tok::Arrow
                }
                '-' if self.peek(1) == Some('-') => {
                    self.bump();
                    self.bump();
                    Tok::Line
                }
                c if c == '-' || c == '.' || c.is_ascii_digit() => {
                    let mut s = String::new();
                    if c == '-' {
                        s.push('-');
                        self.bump();
                    }
                    while let Some(d) = self.peek(0) {
                        if d.is_ascii_digit() || d == '.' {
                            s.push(d);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    Tok::Id { text: s, quoted: false }
                }
                c if is_id_start(c) => {
                    let mut s = String::new();
                    while let Some(d) = self.peek(0) {
                        if is_id_char(d) {
                            s.push(d);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    Tok::Id { text: s, quoted: false }
                }
                other => {
                    return Err(at(line, col, format!("unexpected character {other:?}")));
                }
            };
            out.push(Lexed { tok, line, col });
        }
    }
}

// --- parser ---

/// The attribute defaults in force at one nesting level. A `subgraph` gets
/// its own copy, so `node [shape=diamond]` inside one does not leak out —
/// the *grouping* is ignored in v1, but the scoping is free and surprising
/// to get wrong.
#[derive(Clone, Default)]
struct Defaults {
    node: NodeAttrs,
    edge_color: Option<Rgb>,
}

#[derive(Clone, Default)]
struct NodeAttrs {
    label: Option<String>,
    color: Option<Rgb>,
    shape: Option<NodeShape>,
    /// `style=rounded` seen (`Some(true)`) or a `style=` without it
    /// (`Some(false)`); DOT spells a rounded box as a style, not a shape.
    rounded: Option<bool>,
}

impl NodeAttrs {
    fn merge(&self, over: &NodeAttrs) -> NodeAttrs {
        NodeAttrs {
            label: over.label.clone().or_else(|| self.label.clone()),
            color: over.color.or(self.color),
            shape: over.shape.or(self.shape),
            rounded: over.rounded.or(self.rounded),
        }
    }

    /// The drawn silhouette: the shape, adjusted by `style=rounded`, which
    /// is how DOT asks for a rounded box.
    fn resolve_shape(&self) -> NodeShape {
        match (self.shape.unwrap_or_default(), self.rounded) {
            (NodeShape::Box, Some(true)) => NodeShape::Rounded,
            (NodeShape::Rounded, Some(false)) => NodeShape::Box,
            (s, _) => s,
        }
    }
}

struct Parser {
    toks: Vec<Lexed>,
    i: usize,
    directed: bool,
    rankdir: RankDir,
    name: Option<String>,
    nodes: Vec<DotNode>,
    /// Node id → index, so an edge to an undeclared name declares it.
    index: Vec<(String, usize)>,
    edges: Vec<DotEdge>,
    /// Attributes each node was declared with, so a later `x [color=red]`
    /// updates the node rather than replacing what came before it.
    attrs: Vec<NodeAttrs>,
}

/// Where the last token ended, for an error about what should have followed
/// it. An empty file reports 1:1.
fn tail_pos(toks: &[Lexed]) -> (usize, usize) {
    toks.last().map_or((1, 1), |t| (t.line, t.col))
}

impl Parser {
    fn peek(&self) -> Option<&Lexed> {
        self.toks.get(self.i)
    }

    fn pos(&self) -> (usize, usize) {
        self.peek().map_or_else(|| tail_pos(&self.toks), |t| (t.line, t.col))
    }

    fn eat_punct(&mut self, c: char) -> bool {
        if matches!(self.peek(), Some(Lexed { tok: Tok::Punct(p), .. }) if *p == c) {
            self.i += 1;
            return true;
        }
        false
    }

    fn expect_punct(&mut self, c: char) -> Result<(), BindError> {
        if self.eat_punct(c) {
            return Ok(());
        }
        let (line, col) = self.pos();
        Err(at(line, col, format!("expected {c:?}")))
    }

    /// The next token as an identifier, if it is one.
    fn peek_id(&self) -> Option<(&str, bool)> {
        match self.peek() {
            Some(Lexed { tok: Tok::Id { text, quoted }, .. }) => Some((text.as_str(), *quoted)),
            _ => None,
        }
    }

    /// Is the next token this *bare* keyword? A quoted `"graph"` is a node
    /// name and must not be mistaken for one.
    fn peek_keyword(&self, word: &str) -> bool {
        matches!(self.peek_id(), Some((t, false)) if t.eq_ignore_ascii_case(word))
    }

    fn take_id(&mut self) -> Result<String, BindError> {
        match self.peek_id() {
            Some((text, _)) => {
                let text = text.to_string();
                self.i += 1;
                Ok(text)
            }
            None => {
                let (line, col) = self.pos();
                Err(at(line, col, "expected a name"))
            }
        }
    }

    /// Declare `id` if it is new, and return its index either way.
    fn node_index(&mut self, id: &str) -> usize {
        if let Some((_, i)) = self.index.iter().find(|(n, _)| n == id) {
            return *i;
        }
        let i = self.nodes.len();
        self.nodes.push(DotNode {
            id: id.to_string(),
            label: id.to_string(),
            color: None,
            shape: NodeShape::default(),
        });
        self.attrs.push(NodeAttrs::default());
        self.index.push((id.to_string(), i));
        i
    }

    /// `[ a=b, c=d ] [ e=f ]` — DOT allows several bracket groups in a row.
    /// Returns the pairs in source order; an empty vec when there are none.
    fn attr_list(&mut self) -> Result<Vec<(String, String, usize, usize)>, BindError> {
        let mut out = Vec::new();
        while self.eat_punct('[') {
            loop {
                if self.eat_punct(']') {
                    break;
                }
                let (line, col) = self.pos();
                let key = self.take_id()?;
                // A bare `[rounded]` style word is legal DOT shorthand in
                // some dialects; treat a valueless entry as `key=key` so
                // `[style=filled, rounded]` does not die on the comma.
                let value = if self.eat_punct('=') { self.take_id()? } else { key.clone() };
                out.push((key, value, line, col));
                if !self.eat_punct(',') && self.peek().is_none() {
                    let (l, c) = self.pos();
                    return Err(at(l, c, "expected ']'"));
                }
            }
        }
        Ok(out)
    }

    /// Fold one attribute list into a node-attribute set. Unknown keys are
    /// ignored — a DOT file in the wild is mostly typography.
    fn node_attrs(&self, pairs: &[(String, String, usize, usize)]) -> Result<NodeAttrs, BindError> {
        let mut a = NodeAttrs::default();
        for (key, value, line, col) in pairs {
            match key.to_ascii_lowercase().as_str() {
                "label" => a.label = Some(value.clone()),
                // A fill and an outline are one thing at terminal
                // resolution, so the last of the two named wins.
                "color" | "fillcolor" => {
                    a.color = Some(parse_color(value).map_err(|e| at(*line, *col, e.msg))?)
                }
                "shape" => {
                    a.shape =
                        Some(NodeShape::parse(&value.to_ascii_lowercase()).ok_or_else(|| {
                            at(
                                *line,
                                *col,
                                format!(
                                    "unknown node shape {value:?}; expected one of {}",
                                    NodeShape::NAMES.join(", ")
                                ),
                            )
                        })?)
                }
                "style" => {
                    a.rounded =
                        Some(value.split(',').any(|s| s.trim().eq_ignore_ascii_case("rounded")))
                }
                _ => {}
            }
        }
        Ok(a)
    }

    /// Graph-level attributes: only `rankdir` means anything in v1.
    fn graph_attrs(&mut self, pairs: &[(String, String, usize, usize)]) -> Result<(), BindError> {
        for (key, value, line, col) in pairs {
            if key.eq_ignore_ascii_case("rankdir") {
                self.rankdir = RankDir::parse(value).ok_or_else(|| {
                    at(
                        *line,
                        *col,
                        format!(
                            "unknown rankdir {value:?}; expected one of {}",
                            RankDir::NAMES.join(", ")
                        ),
                    )
                })?;
            }
        }
        Ok(())
    }

    /// One statement. Returns `false` at the closing brace.
    fn stmt(&mut self, defaults: &mut Defaults) -> Result<bool, BindError> {
        if self.peek().is_none() || self.eat_punct('}') {
            return Ok(false);
        }
        // Separators are optional and may repeat.
        if self.eat_punct(';') {
            return Ok(true);
        }
        for word in ["node", "edge", "graph"] {
            if self.peek_keyword(word) {
                // `graph [rankdir=LR]` is a default block; `graph -> x` is a
                // node named graph, which real DOT forbids anyway.
                let save = self.i;
                self.i += 1;
                if matches!(self.peek(), Some(Lexed { tok: Tok::Punct('['), .. })) {
                    let pairs = self.attr_list()?;
                    match word {
                        "node" => defaults.node = defaults.node.merge(&self.node_attrs(&pairs)?),
                        "edge" => {
                            if let Some(c) = self.node_attrs(&pairs)?.color {
                                defaults.edge_color = Some(c);
                            }
                        }
                        _ => self.graph_attrs(&pairs)?,
                    }
                    return Ok(true);
                }
                self.i = save;
                break;
            }
        }
        if self.peek_keyword("subgraph")
            || matches!(self.peek(), Some(Lexed { tok: Tok::Punct('{'), .. }))
        {
            // Grouping is ignored in v1 — a cluster box is a later feature —
            // but the contents are real statements and are hoisted, and the
            // defaults inside are scoped to it.
            if self.peek_keyword("subgraph") {
                self.i += 1;
                if self.peek_id().is_some() {
                    self.i += 1;
                }
            }
            self.expect_punct('{')?;
            let mut inner = defaults.clone();
            while self.stmt(&mut inner)? {}
            return Ok(true);
        }

        // From here it is a node, an edge chain, or `key = value`.
        let (line, col) = self.pos();
        let first = self.take_id()?;
        if self.eat_punct('=') {
            let value = self.take_id()?;
            self.graph_attrs(&[(first, value, line, col)])?;
            return Ok(true);
        }
        let mut chain: Vec<Vec<usize>> = vec![vec![self.node_index(&first)]];
        let mut is_chain = false;
        loop {
            let (line, col) = self.pos();
            match self.peek().map(|t| t.tok.clone()) {
                Some(Tok::Arrow) if self.directed => self.i += 1,
                Some(Tok::Line) if !self.directed => self.i += 1,
                Some(Tok::Arrow) => {
                    return Err(at(line, col, "'->' joins nodes in a digraph; a graph uses '--'"))
                }
                Some(Tok::Line) => {
                    return Err(at(line, col, "'--' joins nodes in a graph; a digraph uses '->'"))
                }
                _ => break,
            }
            is_chain = true;
            chain.push(self.endpoint(defaults)?);
        }

        let pairs = self.attr_list()?;
        if is_chain {
            let color = self.node_attrs(&pairs)?.color.or(defaults.edge_color);
            // `a -> b -> c` is two edges, and `a -> {b c}` fans out: each
            // consecutive pair of endpoint *sets* is joined completely.
            for pair in chain.windows(2) {
                for &from in &pair[0] {
                    for &to in &pair[1] {
                        self.edges.push(DotEdge { from: from as u32, to: to as u32, color });
                    }
                }
            }
        } else {
            // A bare `x [attrs]` declares or updates one node. Declaring it
            // twice merges, so `x [label="A"]; x [color=red]` is one node.
            let i = chain[0][0];
            let merged = defaults.node.merge(&self.attrs[i]).merge(&self.node_attrs(&pairs)?);
            self.attrs[i] = merged;
        }
        Ok(true)
    }

    /// One side of an edge: a node, or a braced set that fans out.
    fn endpoint(&mut self, defaults: &Defaults) -> Result<Vec<usize>, BindError> {
        if self.peek_keyword("subgraph")
            || matches!(self.peek(), Some(Lexed { tok: Tok::Punct('{'), .. }))
        {
            if self.peek_keyword("subgraph") {
                self.i += 1;
                if self.peek_id().is_some() {
                    self.i += 1;
                }
            }
            self.expect_punct('{')?;
            let mut out = Vec::new();
            // An endpoint set is a list of names, not statements: it cannot
            // change the defaults, only read the ones already in force.
            let inner = defaults.clone();
            loop {
                if self.eat_punct('}') {
                    break;
                }
                if self.eat_punct(';') || self.eat_punct(',') {
                    continue;
                }
                if self.peek().is_none() {
                    let (l, c) = self.pos();
                    return Err(at(l, c, "expected '}'"));
                }
                let id = self.take_id()?;
                let i = self.node_index(&id);
                let pairs = self.attr_list()?;
                self.attrs[i] = inner.node.merge(&self.attrs[i]).merge(&self.node_attrs(&pairs)?);
                out.push(i);
            }
            return Ok(out);
        }
        let id = self.take_id()?;
        Ok(vec![self.node_index(&id)])
    }
}

/// Parse the DOT subset. Everything outside it is a
/// [`BindError`](crate::BindError) naming the `line:col` it gave up at and
/// what it expected instead.
///
/// Accepted: `[strict] (graph|digraph) [name] { … }` with `--` edges in a
/// `graph` and `->` in a `digraph`; node statements, edge chains (`a -> b ->
/// c`) and braced fan-outs (`a -> {b c}`); `node`/`edge`/`graph` attribute
/// defaults; `rankdir` at graph level; `subgraph`s, whose contents are
/// hoisted and whose grouping is ignored; `label`, `color`, `fillcolor`,
/// `shape` and `style=rounded` on nodes and `color` on edges, with every
/// other attribute ignored; `//`, `/* */` and `#` comments.
///
/// Rejected with a message: HTML labels, node ports, an edge operator that
/// disagrees with the graph kind, and any unknown shape, colour or rankdir.
pub fn parse_dot(text: &str) -> Result<DotGraph, BindError> {
    let toks = Lexer::new(text).tokens()?;
    let mut p = Parser {
        toks,
        i: 0,
        directed: true,
        rankdir: RankDir::TB,
        name: None,
        nodes: Vec::new(),
        index: Vec::new(),
        edges: Vec::new(),
        attrs: Vec::new(),
    };
    if p.peek_keyword("strict") {
        p.i += 1;
    }
    p.directed = if p.peek_keyword("digraph") {
        p.i += 1;
        true
    } else if p.peek_keyword("graph") {
        p.i += 1;
        false
    } else {
        let (line, col) = p.pos();
        return Err(at(line, col, "expected 'graph' or 'digraph'"));
    };
    if p.peek_id().is_some() && !matches!(p.peek(), Some(Lexed { tok: Tok::Punct('{'), .. })) {
        p.name = Some(p.take_id()?);
    }
    p.expect_punct('{')?;
    let mut defaults = Defaults::default();
    while p.stmt(&mut defaults)? {}

    let Parser { name, directed, rankdir, mut nodes, edges, attrs, .. } = p;
    for (node, a) in nodes.iter_mut().zip(&attrs) {
        if let Some(label) = &a.label {
            node.label = label.clone();
        }
        node.color = a.color;
        node.shape = a.resolve_shape();
    }
    Ok(DotGraph { name, directed, rankdir, nodes, edges })
}

/// Parse DOT, lay it out, and return a plot ready to render, the graph
/// trace's handle, and the parse itself — hosts need the node ids and
/// labels to say what a hover landed on.
///
/// `rankdir` overrides whatever the document asked for; `None` honours the
/// document (and `TB` when it says nothing). Nodes without a `color` take
/// the plot's first colorway slot, so an uncoloured graph still reads as one
/// series rather than as eight.
pub fn plot_from_dot(
    text: &str,
    rankdir: Option<RankDir>,
) -> Result<(Plot, TraceId, DotGraph), BindError> {
    let g = parse_dot(text)?;
    let dir = rankdir.unwrap_or(g.rankdir);
    let edges: Vec<(u32, u32)> = g.edges.iter().map(|e| (e.from, e.to)).collect();
    let layout = LayeredLayout::new(g.nodes.len(), &edges, dir);
    let (pts, starts) = layout.routes();

    let accent = COLORWAY_PLOTUI[0];
    let mut plot = Plot::new();
    let any_edge_color = g.edges.iter().any(|e| e.color.is_some());
    let handle = plot.add_graph2d(
        layout.positions().to_vec(),
        g.nodes.iter().map(|n| n.label.clone()).collect(),
        g.nodes.iter().map(|n| n.color.unwrap_or(accent)).collect(),
        edges,
        g.directed,
        Some(g.nodes.iter().map(|n| n.shape).collect()),
        // Only pinned when the document coloured at least one edge; without
        // that the renderer's dimmed endpoint average is the better default.
        any_edge_color.then(|| g.edges.iter().map(|e| e.color.unwrap_or([90, 96, 112])).collect()),
        Some((pts.to_vec(), starts.to_vec())),
        g.name.clone(),
    );
    // A layout's coordinates are not measurements; pinned rather than left
    // to the automatic rule so adding a second trace does not bring a
    // meaningless numeric ladder back with it.
    plot.set_show_axes(false);
    Ok((plot, handle, g))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(text: &str) -> DotGraph {
        parse_dot(text).expect("should parse")
    }

    fn ids(g: &DotGraph) -> Vec<&str> {
        g.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    fn pairs(g: &DotGraph) -> Vec<(u32, u32)> {
        g.edges.iter().map(|e| (e.from, e.to)).collect()
    }

    #[test]
    fn the_smallest_useful_document_parses() {
        let d = g("digraph { a -> b }");
        assert!(d.directed);
        assert_eq!(d.name, None);
        assert_eq!(ids(&d), ["a", "b"]);
        assert_eq!(pairs(&d), [(0, 1)]);
        assert_eq!(d.nodes[0].label, "a", "an unlabelled node is labelled by its id");
        assert_eq!(d.nodes[0].shape, NodeShape::Rounded);
    }

    #[test]
    fn chains_expand_to_pairs_and_fan_outs_join_every_pair() {
        assert_eq!(pairs(&g("digraph { a -> b -> c -> d }")), [(0, 1), (1, 2), (2, 3)]);
        let d = g("digraph { build -> { test lint } -> ship }");
        assert_eq!(ids(&d), ["build", "test", "lint", "ship"]);
        assert_eq!(pairs(&d), [(0, 1), (0, 2), (1, 3), (2, 3)]);
    }

    #[test]
    fn declaration_order_is_index_order_and_edges_declare_new_names() {
        let d = g("digraph { z; a -> z; b }");
        assert_eq!(ids(&d), ["z", "a", "b"]);
        assert_eq!(pairs(&d), [(1, 0)]);
    }

    #[test]
    fn node_attributes_set_label_colour_and_shape() {
        let d = g(r##"digraph {
            a [label="Fetch prices", color=red, shape=box]
            b [fillcolor="#45c8d1", shape=diamond]
            c [shape=box, style=rounded]
            d [shape=circle]
        }"##);
        assert_eq!(d.nodes[0].label, "Fetch prices");
        assert_eq!(d.nodes[0].color, Some([255, 0, 0]));
        assert_eq!(d.nodes[0].shape, NodeShape::Box);
        assert_eq!(d.nodes[1].color, Some([69, 200, 209]), "fillcolor is a colour like any other");
        assert_eq!(d.nodes[1].shape, NodeShape::Diamond);
        assert_eq!(d.nodes[2].shape, NodeShape::Rounded, "style=rounded rounds a box");
        assert_eq!(d.nodes[3].shape, NodeShape::Ellipse, "a circle sized to a label is an ellipse");
    }

    #[test]
    fn defaults_apply_to_later_nodes_and_scope_to_a_subgraph() {
        let d = g(r#"digraph {
            node [shape=box, color=teal]
            a
            subgraph cluster_x { node [shape=diamond]; b }
            c
            edge [color=orange]
            a -> c
        }"#);
        assert_eq!(ids(&d), ["a", "b", "c"]);
        assert_eq!(d.nodes[0].shape, NodeShape::Box);
        assert_eq!(d.nodes[0].color, Some([0, 128, 128]));
        assert_eq!(d.nodes[1].shape, NodeShape::Diamond, "the subgraph's default applies inside");
        assert_eq!(d.nodes[2].shape, NodeShape::Box, "and does not leak out of it");
        assert_eq!(d.edges[0].color, Some([255, 165, 0]));
    }

    #[test]
    fn subgraph_contents_are_hoisted_and_cluster_names_are_not_an_error() {
        let d = g("digraph { subgraph cluster_etl { x -> y } ; y -> z }");
        assert_eq!(ids(&d), ["x", "y", "z"]);
        assert_eq!(pairs(&d), [(0, 1), (1, 2)]);
    }

    #[test]
    fn rankdir_is_accepted_bare_and_as_a_graph_attribute() {
        assert_eq!(g("digraph { rankdir=LR; a -> b }").rankdir, RankDir::LR);
        assert_eq!(g("digraph { graph [rankdir=lr]; a -> b }").rankdir, RankDir::LR);
        assert_eq!(g("digraph { a -> b }").rankdir, RankDir::TB, "TB is the default");
    }

    #[test]
    fn comments_separators_and_quoting_all_work() {
        let d = g(r#"
            // a leading comment
            strict digraph pipeline {   # and a hash one
              /* and a
                 block one */
              "with spaces" [label="quoted \"label\""];
              "with spaces" -> plain
            }
        "#);
        assert_eq!(d.name.as_deref(), Some("pipeline"));
        assert_eq!(ids(&d), ["with spaces", "plain"]);
        assert_eq!(d.nodes[0].label, r#"quoted "label""#);
    }

    #[test]
    fn an_undirected_graph_uses_the_other_operator() {
        let d = g("graph friends { a -- b -- c }");
        assert!(!d.directed);
        assert_eq!(pairs(&d), [(0, 1), (1, 2)]);
    }

    #[test]
    fn parse_errors_carry_the_canonical_messages() {
        let msg = |t: &str| parse_dot(t).unwrap_err().msg;
        assert_eq!(
            msg("digraph { a -- b }"),
            "1:13: '--' joins nodes in a graph; a digraph uses '->'"
        );
        assert_eq!(
            msg("graph { a -> b }"),
            "1:11: '->' joins nodes in a digraph; a graph uses '--'"
        );
        assert_eq!(
            msg("digraph { a [label=<b>b</b>] }"),
            "1:20: HTML labels (<...>) are not supported; use a quoted string"
        );
        assert_eq!(
            msg("digraph { a:head -> b }"),
            "1:12: node ports (a:port) are not supported; drop the ':port'"
        );
        assert_eq!(
            msg("digraph { a [shape=blob] }"),
            "1:14: unknown node shape \"blob\"; expected one of rounded, box, ellipse, diamond"
        );
        assert!(msg("digraph { a [color=blurple] }").starts_with("1:14: unknown color"));
        assert_eq!(
            msg("digraph { rankdir=sideways }"),
            "1:11: unknown rankdir \"sideways\"; expected one of TB, LR"
        );
        assert_eq!(msg("something { }"), "1:1: expected 'graph' or 'digraph'");
        assert_eq!(msg("digraph"), "1:1: expected '{'");
        assert_eq!(msg("digraph { \"oops }"), "1:11: unterminated string");
        assert_eq!(msg("digraph { /* oops }"), "1:11: unterminated /* comment");
    }

    #[test]
    fn plot_from_dot_builds_a_hidden_axes_plot() {
        let (plot, handle, doc) =
            plot_from_dot("digraph nightly { a -> b -> c; a -> c }", None).unwrap();
        assert_eq!(handle, 0);
        assert_eq!(doc.nodes.len(), 3);
        assert_eq!(plot.show_axes, Some(false), "a graph's coordinates are not a scale");
        assert_eq!(plot.node_count(), 3);
        // The skipping edge is the third one and must have been routed.
        let (_, starts) = match &plot.traces[handle] {
            plotui_core::Trace::Graph2d { route_pts, route_starts, .. } => {
                (route_pts.clone(), route_starts.clone())
            }
            _ => panic!("expected a graph2d trace"),
        };
        assert_eq!(starts.len(), 3, "one CSR run per edge");
        assert_eq!(plot.render(300, 220).w, 300, "and it renders");
    }

    #[test]
    fn plot_from_dot_honours_and_overrides_rankdir() {
        let lr = plot_from_dot("digraph { rankdir=LR; a -> b }", None).unwrap().0;
        let forced = plot_from_dot("digraph { rankdir=LR; a -> b }", Some(RankDir::TB)).unwrap().0;
        let xs = |p: &Plot| match &p.traces[0] {
            plotui_core::Trace::Graph2d { nodes, .. } => nodes.clone(),
            _ => panic!("expected a graph2d trace"),
        };
        // LR spreads along x, TB along y.
        assert!(xs(&lr)[0][0] < xs(&lr)[1][0]);
        assert!(xs(&forced)[0][1] > xs(&forced)[1][1]);
    }
}
