//! Text with positions from a PDF page: a small interpreter of the content
//! operators that place text (graphics and text matrices, fonts, form
//! XObjects), enough for the House's generated reports. Each character comes
//! out with where it starts and ends on the page, in PDF points with y up.

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;

/// One run of text the page draws, placed on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub x: f64,
    /// Where the text after it would start (x + its advance).
    pub x_end: f64,
    pub y: f64,
    /// Font size on the page, in points.
    pub size: f64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct M([f64; 6]);

impl M {
    const ID: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    /// self × other (apply self, then other), as PDF writes matrix products.
    fn then(self, o: M) -> M {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        M([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            e * a2 + f * c2 + e2,
            e * b2 + f * d2 + f2,
        ])
    }
    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    fn translate(tx: f64, ty: f64) -> M {
        M([1.0, 0.0, 0.0, 1.0, tx, ty])
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn matrix(ops: &[Object]) -> Option<M> {
    if ops.len() < 6 {
        return None;
    }
    let v: Vec<f64> = ops.iter().take(6).filter_map(num).collect();
    (v.len() == 6).then(|| M([v[0], v[1], v[2], v[3], v[4], v[5]]))
}

fn dict<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => match doc.get_object(*id).ok()? {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        },
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn stream_bytes(doc: &Document, id: ObjectId) -> Option<Vec<u8>> {
    match doc.get_object(id).ok()? {
        Object::Stream(s) => Some(
            s.decompressed_content()
                .unwrap_or_else(|_| s.content.clone()),
        ),
        _ => None,
    }
}

/// What a font needs to turn codes into text and advances.
struct Font {
    /// Two-byte codes (Type0, Identity-H) or one-byte.
    two_bytes: bool,
    to_unicode: HashMap<u32, String>,
    /// Glyph widths in 1/1000 of the font size.
    widths: HashMap<u32, f64>,
    default_width: f64,
}

fn hex_code(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim_matches(['<', '>']), 16).ok()
}

fn utf16_hex(s: &str) -> String {
    let h = s.trim_matches(['<', '>']);
    let units: Vec<u16> = (0..h.len() / 4)
        .filter_map(|i| u16::from_str_radix(&h[i * 4..i * 4 + 4], 16).ok())
        .collect();
    String::from_utf16_lossy(&units)
        .chars()
        .filter(|c| *c != '\0')
        .collect()
}

/// A ToUnicode CMap's `bfchar` and `bfrange` entries.
pub fn parse_cmap(text: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let mut rest = text;
    while let Some(i) = rest.find("beginbf") {
        rest = &rest[i + "beginbf".len()..];
        let is_range = rest.starts_with("range");
        let Some(end) = rest.find("endbf") else { break };
        let block = &rest[..end];
        rest = &rest[end..];
        // Tokens: <hex> or [ ... ] arrays.
        let mut tokens: Vec<String> = Vec::new();
        let mut chars = block.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '<' {
                let j = block[i..]
                    .find('>')
                    .map(|j| i + j + 1)
                    .unwrap_or(block.len());
                tokens.push(block[i..j].to_string());
                while chars.peek().is_some_and(|(k, _)| *k < j) {
                    chars.next();
                }
            } else if c == '[' {
                let j = block[i..]
                    .find(']')
                    .map(|j| i + j + 1)
                    .unwrap_or(block.len());
                tokens.push(block[i..j].to_string());
                while chars.peek().is_some_and(|(k, _)| *k < j) {
                    chars.next();
                }
            }
        }
        if is_range {
            for t in tokens.chunks(3) {
                let [lo, hi, dst] = t else { continue };
                let (Some(lo), Some(hi)) = (hex_code(lo), hex_code(hi)) else {
                    continue;
                };
                if hi < lo || hi - lo > 65_535 {
                    continue;
                }
                if dst.starts_with('[') {
                    let items: Vec<&str> = dst
                        .trim_matches(['[', ']'])
                        .split('>')
                        .map(|s| s.trim().trim_start_matches('<'))
                        .filter(|s| !s.is_empty())
                        .collect();
                    for (k, h) in items.iter().enumerate() {
                        map.insert(lo + k as u32, utf16_hex(h));
                    }
                } else {
                    let base = dst.trim_matches(['<', '>']);
                    let Ok(start) = u32::from_str_radix(base, 16) else {
                        continue;
                    };
                    for code in lo..=hi {
                        if let Some(ch) = char::from_u32(start + (code - lo)) {
                            if ch != '\0' {
                                map.insert(code, ch.to_string());
                            } else {
                                map.insert(code, String::new());
                            }
                        }
                    }
                }
            }
        } else {
            for t in tokens.chunks(2) {
                let [src, dst] = t else { continue };
                if let Some(code) = hex_code(src) {
                    map.insert(code, utf16_hex(dst));
                }
            }
        }
    }
    map
}

fn load_font(doc: &Document, font: &Dictionary) -> Font {
    let subtype = font
        .get(b"Subtype")
        .and_then(|o| o.as_name())
        .unwrap_or(b"");
    let two_bytes = subtype == b"Type0";
    let to_unicode = font
        .get(b"ToUnicode")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .and_then(|id| stream_bytes(doc, id))
        .map(|b| parse_cmap(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let mut widths = HashMap::new();
    let mut default_width = if two_bytes { 1000.0 } else { 500.0 };
    if two_bytes {
        let desc = font.get(b"DescendantFonts").ok().and_then(|o| match o {
            Object::Array(a) => a.first().cloned(),
            Object::Reference(id) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first().cloned()),
            _ => None,
        });
        if let Some(d) = desc.as_ref().and_then(|o| dict(doc, o)) {
            if let Some(dw) = d.get(b"DW").ok().and_then(num) {
                default_width = dw;
            }
            let w = d.get(b"W").ok().and_then(|o| match o {
                Object::Array(a) => Some(a.clone()),
                Object::Reference(id) => doc
                    .get_object(*id)
                    .ok()
                    .and_then(|o| o.as_array().ok().cloned()),
                _ => None,
            });
            if let Some(w) = w {
                let mut i = 0;
                while i < w.len() {
                    let Some(first) = num(&w[i]) else { break };
                    match w.get(i + 1) {
                        Some(Object::Array(list)) => {
                            for (k, v) in list.iter().enumerate() {
                                if let Some(v) = num(v) {
                                    widths.insert(first as u32 + k as u32, v);
                                }
                            }
                            i += 2;
                        }
                        Some(last) => {
                            if let (Some(last), Some(v)) = (num(last), w.get(i + 2).and_then(num)) {
                                for c in first as u32..=last as u32 {
                                    widths.insert(c, v);
                                }
                            }
                            i += 3;
                        }
                        None => break,
                    }
                }
            }
        }
    } else {
        let first = font.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as u32;
        if let Some(Object::Array(ws)) = font.get(b"Widths").ok().map(|o| match o {
            Object::Reference(id) => doc.get_object(*id).cloned().unwrap_or(Object::Null),
            other => other.clone(),
        }) {
            for (k, v) in ws.iter().enumerate() {
                if let Some(v) = num(v) {
                    widths.insert(first + k as u32, v);
                }
            }
        }
    }
    Font {
        two_bytes,
        to_unicode,
        widths,
        default_width,
    }
}

impl Font {
    /// The text and the advance (in text space, before the font size) of a string.
    fn decode(&self, bytes: &[u8]) -> Vec<(String, f64, bool)> {
        let codes: Vec<u32> = if self.two_bytes {
            bytes
                .chunks(2)
                .map(|c| ((c[0] as u32) << 8) | *c.get(1).unwrap_or(&0) as u32)
                .collect()
        } else {
            bytes.iter().map(|b| *b as u32).collect()
        };
        codes
            .into_iter()
            .map(|c| {
                let text = match self.to_unicode.get(&c) {
                    Some(t) => t.clone(),
                    None if !self.two_bytes => (c as u8 as char).to_string(),
                    None => String::new(),
                };
                let w = self.widths.get(&c).copied().unwrap_or(self.default_width) / 1000.0;
                // Word spacing applies to the one-byte code 32 only.
                let is_space = !self.two_bytes && c == 32;
                (text, w, is_space)
            })
            .collect()
    }
}

struct State {
    ctm: M,
}

struct TextState {
    tm: M,
    tlm: M,
    font: Option<String>,
    size: f64,
    leading: f64,
    char_spacing: f64,
    word_spacing: f64,
    hscale: f64,
    rise: f64,
}

/// Every text run the page draws, in page points (y up), page by page.
pub fn page_items(doc: &Document) -> Vec<Vec<Item>> {
    let mut pages = Vec::new();
    for (_, page_id) in doc.get_pages() {
        let mut items = Vec::new();
        let Ok(Object::Dictionary(page)) = doc.get_object(page_id) else {
            pages.push(items);
            continue;
        };
        let resources = page
            .get(b"Resources")
            .ok()
            .and_then(|o| dict(doc, o))
            .cloned()
            .unwrap_or_default();
        let content = doc.get_page_content(page_id);
        let mut fonts: HashMap<ObjectId, Font> = HashMap::new();
        run(doc, &content, &resources, M::ID, &mut items, &mut fonts, 0);
        pages.push(items);
    }
    pages
}

fn font_id(doc: &Document, resources: &Dictionary, name: &[u8]) -> Option<ObjectId> {
    let fonts = resources.get(b"Font").ok().and_then(|o| dict(doc, o))?;
    fonts.get(name).ok()?.as_reference().ok()
}

fn run(
    doc: &Document,
    content: &[u8],
    resources: &Dictionary,
    base: M,
    items: &mut Vec<Item>,
    fonts: &mut HashMap<ObjectId, Font>,
    depth: usize,
) {
    if depth > 8 {
        return;
    }
    let Ok(ops) = lopdf::content::Content::decode(content) else {
        return;
    };
    let mut stack: Vec<State> = Vec::new();
    let mut gs = State { ctm: base };
    let mut ts = TextState {
        tm: M::ID,
        tlm: M::ID,
        font: None,
        size: 0.0,
        leading: 0.0,
        char_spacing: 0.0,
        word_spacing: 0.0,
        hscale: 1.0,
        rise: 0.0,
    };
    let mut font_ref: Option<ObjectId> = None;
    for op in &ops.operations {
        let a = &op.operands;
        match op.operator.as_str() {
            "q" => stack.push(State { ctm: gs.ctm }),
            "Q" => {
                if let Some(s) = stack.pop() {
                    gs = s;
                }
            }
            "cm" => {
                if let Some(m) = matrix(a) {
                    gs.ctm = m.then(gs.ctm);
                }
            }
            "BT" => {
                ts.tm = M::ID;
                ts.tlm = M::ID;
            }
            "Tf" => {
                if let (Some(Object::Name(n)), Some(s)) = (a.first(), a.get(1).and_then(num)) {
                    ts.font = Some(String::from_utf8_lossy(n).into_owned());
                    ts.size = s;
                    font_ref = font_id(doc, resources, n);
                    if let Some(id) = font_ref {
                        if let std::collections::hash_map::Entry::Vacant(e) = fonts.entry(id) {
                            if let Some(d) = doc.get_object(id).ok().and_then(|o| o.as_dict().ok())
                            {
                                e.insert(load_font(doc, d));
                            }
                        }
                    }
                }
            }
            "Tc" => ts.char_spacing = a.first().and_then(num).unwrap_or(0.0),
            "Tw" => ts.word_spacing = a.first().and_then(num).unwrap_or(0.0),
            "Tz" => ts.hscale = a.first().and_then(num).unwrap_or(100.0) / 100.0,
            "TL" => ts.leading = a.first().and_then(num).unwrap_or(0.0),
            "Ts" => ts.rise = a.first().and_then(num).unwrap_or(0.0),
            "Td" | "TD" => {
                if let (Some(tx), Some(ty)) = (a.first().and_then(num), a.get(1).and_then(num)) {
                    if op.operator == "TD" {
                        ts.leading = -ty;
                    }
                    ts.tlm = M::translate(tx, ty).then(ts.tlm);
                    ts.tm = ts.tlm;
                }
            }
            "Tm" => {
                if let Some(m) = matrix(a) {
                    ts.tlm = m;
                    ts.tm = m;
                }
            }
            "T*" => {
                ts.tlm = M::translate(0.0, -ts.leading).then(ts.tlm);
                ts.tm = ts.tlm;
            }
            "Tj" | "'" | "\"" | "TJ" => {
                if op.operator == "'" || op.operator == "\"" {
                    ts.tlm = M::translate(0.0, -ts.leading).then(ts.tlm);
                    ts.tm = ts.tlm;
                }
                let Some(font) = font_ref.and_then(|id| fonts.get(&id)) else {
                    continue;
                };
                let parts: Vec<Object> = match op.operator.as_str() {
                    "TJ" => a
                        .first()
                        .and_then(|o| o.as_array().ok())
                        .cloned()
                        .unwrap_or_default(),
                    "\"" => a.get(2).cloned().into_iter().collect(),
                    _ => a.first().cloned().into_iter().collect(),
                };
                for part in parts {
                    match part {
                        Object::String(bytes, _) => {
                            for (text, w, is_space) in font.decode(&bytes) {
                                let trm = M([ts.size * ts.hscale, 0.0, 0.0, ts.size, 0.0, ts.rise])
                                    .then(ts.tm)
                                    .then(gs.ctm);
                                let (x, y) = trm.apply(0.0, 0.0);
                                let adv = (w * ts.size
                                    + ts.char_spacing
                                    + if is_space { ts.word_spacing } else { 0.0 })
                                    * ts.hscale;
                                ts.tm = M::translate(adv, 0.0).then(ts.tm);
                                let (x_end, _) =
                                    M([ts.size * ts.hscale, 0.0, 0.0, ts.size, 0.0, ts.rise])
                                        .then(ts.tm)
                                        .then(gs.ctm)
                                        .apply(0.0, 0.0);
                                // The size on the page: how tall 1 unit of text is.
                                let (_, y1) = trm.apply(0.0, 1.0);
                                if !text.is_empty() {
                                    items.push(Item {
                                        x,
                                        x_end,
                                        y,
                                        size: (y1 - y).abs(),
                                        text,
                                    });
                                }
                            }
                        }
                        other => {
                            if let Some(k) = num(&other) {
                                let adv = -k / 1000.0 * ts.size * ts.hscale;
                                ts.tm = M::translate(adv, 0.0).then(ts.tm);
                            }
                        }
                    }
                }
            }
            "Do" => {
                let Some(Object::Name(n)) = a.first() else {
                    continue;
                };
                let Some(xid) = resources
                    .get(b"XObject")
                    .ok()
                    .and_then(|o| dict(doc, o))
                    .and_then(|x| x.get(n).ok())
                    .and_then(|o| o.as_reference().ok())
                else {
                    continue;
                };
                let Ok(Object::Stream(s)) = doc.get_object(xid) else {
                    continue;
                };
                if s.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() != Some(b"Form".as_slice())
                {
                    continue;
                }
                let m = s
                    .dict
                    .get(b"Matrix")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .and_then(|v| matrix(v))
                    .unwrap_or(M::ID);
                let res = s
                    .dict
                    .get(b"Resources")
                    .ok()
                    .and_then(|o| dict(doc, o))
                    .cloned()
                    .unwrap_or_else(|| resources.clone());
                let bytes = s
                    .decompressed_content()
                    .unwrap_or_else(|_| s.content.clone());
                run(doc, &bytes, &res, m.then(gs.ctm), items, fonts, depth + 1);
            }
            _ => {}
        }
    }
}
