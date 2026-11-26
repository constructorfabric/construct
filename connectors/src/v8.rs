//! The V8 value serialization format (`ValueSerializer`), as far as plain data goes: objects,
//! arrays, maps, sets, strings, numbers, booleans, dates and references to an object read
//! before. A value is read into JSON. Anything else (array buffers, host objects, ...) is an
//! error that names the tag and its offset.

use std::fmt;

use serde_json::{Map, Number, Value};

/// How deep values may nest before the input is refused.
const MAX_DEPTH: usize = 256;
/// The longest sparse array that is filled out.
const MAX_SPARSE_LENGTH: u32 = 1 << 16;
/// The most memory the values read from one input may take, as [`Reader::charge`] counts it:
/// a few bytes of input can stand for much more output (a sparse array, a reference to a large
/// object), so the output is bounded, not the input.
const MAX_OUTPUT_BYTES: usize = 256 << 20;
/// What one value takes, besides the text of a string.
const VALUE_BYTES: usize = std::mem::size_of::<Value>();

/// Why a serialized value could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// What went wrong.
    pub reason: String,
    /// Where in the bytes.
    pub offset: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.reason, self.offset)
    }
}

impl std::error::Error for Error {}

/// Reads one value that starts with the V8 version header.
///
/// # Errors
/// Fails on a missing header, a truncated value, a tag this reader does not know, a nesting
/// deeper than [`MAX_DEPTH`], a reference to an object that is not read yet, or a value larger
/// than [`MAX_OUTPUT_BYTES`].
pub fn read(bytes: &[u8]) -> Result<Value, Error> {
    let mut reader = Reader {
        bytes,
        pos: 0,
        objects: Vec::new(),
        depth: 0,
        size: 0,
    };
    if reader.byte()? != 0xFF {
        return Err(error_at(0, "no V8 version header"));
    }
    reader.varint()?;
    reader.value()
}

/// An object whose reading began: its id, and the output taken before it.
#[derive(Clone, Copy)]
struct Begun {
    id: usize,
    size: usize,
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Every object read so far, by its id, with what it took, for the references to it; `None`
    /// while it is read.
    objects: Vec<Option<(Value, usize)>>,
    depth: usize,
    /// The memory taken so far, see [`Reader::charge`].
    size: usize,
}

fn error_at(offset: usize, reason: &str) -> Error {
    Error {
        reason: reason.to_owned(),
        offset,
    }
}

impl Reader<'_> {
    fn error(&self, reason: &str) -> Error {
        error_at(self.pos, reason)
    }

    /// Counts `bytes` more of output, before they are made.
    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.size = self.size.saturating_add(bytes);
        if self.size > MAX_OUTPUT_BYTES {
            return Err(self.error("value too large"));
        }
        Ok(())
    }

    fn byte(&mut self) -> Result<u8, Error> {
        let byte = *self
            .bytes
            .get(self.pos)
            .ok_or_else(|| self.error("unexpected end"))?;
        self.pos += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&[u8], Error> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| self.error("length past the end"))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn varint(&mut self) -> Result<u64, Error> {
        let start = self.pos;
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(error_at(start, "varint too long"))
    }

    fn varint32(&mut self) -> Result<u32, Error> {
        let start = self.pos;
        let value = self.varint()?;
        u32::try_from(value).map_err(|_| error_at(start, "varint too large"))
    }

    fn length(&mut self) -> Result<usize, Error> {
        let start = self.pos;
        let value = self.varint()?;
        usize::try_from(value).map_err(|_| error_at(start, "length too large"))
    }

    fn double(&mut self) -> Result<f64, Error> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| self.error("short double"))?;
        Ok(f64::from_le_bytes(bytes))
    }

    /// The next tag; padding between values is skipped.
    fn tag(&mut self) -> Result<u8, Error> {
        loop {
            let tag = self.byte()?;
            if tag != 0 {
                return Ok(tag);
            }
        }
    }

    fn peek_tag(&mut self) -> Result<u8, Error> {
        while self.bytes.get(self.pos) == Some(&0) {
            self.pos += 1;
        }
        self.bytes
            .get(self.pos)
            .copied()
            .ok_or_else(|| self.error("unexpected end"))
    }

    fn value(&mut self) -> Result<Value, Error> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("values nest too deep"));
        }
        self.depth += 1;
        let value = self.charge(VALUE_BYTES).and_then(|()| self.value_inner());
        self.depth -= 1;
        value
    }

    fn value_inner(&mut self) -> Result<Value, Error> {
        let mut start = self.pos;
        let mut tag = self.tag()?;
        // The count of objects V8 checks before a value: skipped, as often as it comes.
        while tag == b'?' {
            self.varint()?;
            start = self.pos;
            tag = self.tag()?;
        }
        Ok(match tag {
            b'_' | b'0' | b'-' => Value::Null,
            b'T' => Value::Bool(true),
            b'F' => Value::Bool(false),
            b'I' => {
                let raw = self.varint32()?;
                let int = i64::from(raw >> 1) ^ -i64::from(raw & 1);
                Value::from(int)
            }
            b'U' => Value::from(self.varint32()?),
            b'N' => number(self.double()?),
            b'"' | b'c' | b'S' => Value::String(self.string_of(tag)?),
            b'^' => {
                let id = self.length()?;
                // The copy is counted before it is made.
                let size = self.read_before(id, start)?.1;
                self.charge(size)?;
                return self.read_before(id, start).map(|(value, _)| value.clone());
            }
            b'o' => self.object()?,
            b'A' => self.dense_array()?,
            b'a' => self.sparse_array()?,
            b';' => self.map()?,
            b'\'' => self.set()?,
            // A date is its time in milliseconds; a number object is its number.
            b'D' | b'n' => {
                let begun = self.begin();
                let value = number(self.double()?);
                self.end(begun, &value)?;
                value
            }
            b'y' | b'x' => {
                let begun = self.begin();
                let value = Value::Bool(tag == b'y');
                self.end(begun, &value)?;
                value
            }
            b's' => {
                let begun = self.begin();
                let inner = self.tag()?;
                let value = Value::String(self.string_of(inner)?);
                self.end(begun, &value)?;
                value
            }
            b'R' => {
                let begun = self.begin();
                let inner = self.tag()?;
                let value = Value::String(self.string_of(inner)?);
                self.varint()?;
                self.end(begun, &value)?;
                value
            }
            other => {
                return Err(error_at(start, &format!("unknown tag 0x{other:02x}")));
            }
        })
    }

    /// The object `id` that a reference at `start` names, with what it took.
    fn read_before(&self, id: usize, start: usize) -> Result<&(Value, usize), Error> {
        self.objects
            .get(id)
            .and_then(Option::as_ref)
            .ok_or_else(|| error_at(start, "reference to an object not read"))
    }

    /// Reserves the id of an object that is being read.
    fn begin(&mut self) -> Begun {
        self.objects.push(None);
        Begun {
            id: self.objects.len() - 1,
            size: self.size,
        }
    }

    /// Keeps a copy of a finished object for the references to it; the copy takes as much as the
    /// object took.
    fn end(&mut self, begun: Begun, value: &Value) -> Result<(), Error> {
        let taken = self.size - begun.size;
        self.charge(taken)?;
        if let Some(slot) = self.objects.get_mut(begun.id) {
            *slot = Some((value.clone(), taken));
        }
        Ok(())
    }

    fn string_of(&mut self, tag: u8) -> Result<String, Error> {
        let len = self.length()?;
        self.charge(len)?;
        let start = self.pos;
        let bytes = self.take(len)?;
        Ok(match tag {
            b'"' => bytes.iter().copied().map(char::from).collect(),
            b'S' => String::from_utf8_lossy(bytes).into_owned(),
            b'c' => {
                if len % 2 != 0 {
                    return Err(error_at(start, "two-byte string of odd length"));
                }
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            other => {
                return Err(error_at(start, &format!("not a string tag 0x{other:02x}")));
            }
        })
    }

    /// A property key: a string or a number.
    fn key(&mut self) -> Result<String, Error> {
        let start = self.pos;
        match self.value()? {
            Value::String(key) => Ok(key),
            Value::Number(key) => Ok(key.to_string()),
            _ => Err(error_at(start, "property key that is no string or number")),
        }
    }

    /// Properties up to the `end` tag, then the property count.
    fn properties(&mut self, end: u8, into: &mut Map<String, Value>) -> Result<(), Error> {
        while self.peek_tag()? != end {
            let key = self.key()?;
            let value = self.value()?;
            into.insert(key, value);
        }
        self.tag()?;
        self.varint()?;
        Ok(())
    }

    fn object(&mut self) -> Result<Value, Error> {
        let begun = self.begin();
        let mut map = Map::new();
        self.properties(b'{', &mut map)?;
        let value = Value::Object(map);
        self.end(begun, &value)?;
        Ok(value)
    }

    fn dense_array(&mut self) -> Result<Value, Error> {
        let begun = self.begin();
        let len = self.length()?;
        let mut items = Vec::with_capacity(len.min(1024));
        for _ in 0..len {
            items.push(self.value()?);
        }
        // Properties of the array beyond its elements are not kept.
        let mut extra = Map::new();
        self.properties(b'$', &mut extra)?;
        self.varint()?;
        let value = Value::Array(items);
        self.end(begun, &value)?;
        Ok(value)
    }

    fn sparse_array(&mut self) -> Result<Value, Error> {
        let begun = self.begin();
        let start = self.pos;
        let len = self.varint32()?;
        if len > MAX_SPARSE_LENGTH {
            return Err(error_at(start, "sparse array too long"));
        }
        self.charge(len as usize * VALUE_BYTES)?;
        let mut items = vec![Value::Null; len as usize];
        let mut properties = Map::new();
        self.properties(b'@', &mut properties)?;
        self.varint()?;
        for (key, value) in properties {
            if let Some(slot) = key.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                *slot = value;
            }
        }
        let value = Value::Array(items);
        self.end(begun, &value)?;
        Ok(value)
    }

    fn map(&mut self) -> Result<Value, Error> {
        let begun = self.begin();
        let mut map = Map::new();
        while self.peek_tag()? != b':' {
            let key = self.key()?;
            let value = self.value()?;
            map.insert(key, value);
        }
        self.tag()?;
        self.varint()?;
        let value = Value::Object(map);
        self.end(begun, &value)?;
        Ok(value)
    }

    fn set(&mut self) -> Result<Value, Error> {
        let begun = self.begin();
        let mut items = Vec::new();
        while self.peek_tag()? != b',' {
            items.push(self.value()?);
        }
        self.tag()?;
        self.varint()?;
        let value = Value::Array(items);
        self.end(begun, &value)?;
        Ok(value)
    }
}

/// A double as JSON: a whole number as an integer, NaN and infinities as null.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        #[allow(clippy::cast_possible_truncation)]
        return Value::from(value as i64);
    }
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The V8 header of version 15, then `body`.
    fn v8(body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0x0F];
        bytes.extend_from_slice(body);
        bytes
    }

    fn one_byte(text: &str) -> Vec<u8> {
        let mut bytes = vec![b'"', u8::try_from(text.len()).unwrap()];
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }

    fn two_byte(text: &str) -> Vec<u8> {
        let units: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut bytes = vec![b'c', u8::try_from(units.len()).unwrap()];
        bytes.extend_from_slice(&units);
        bytes
    }

    #[test]
    fn an_object_of_every_plain_kind_reads_as_json() {
        let mut body = vec![b'o'];
        body.extend(one_byte("name"));
        body.extend(one_byte("Berlin"));
        body.extend(one_byte("russian"));
        // Padding before a two-byte string, as V8 writes it to align the characters.
        body.push(0);
        body.extend(two_byte("Берлин"));
        body.extend(one_byte("count"));
        body.extend([b'I', 0x0E]); // 7
        body.extend(one_byte("negative"));
        body.extend([b'I', 0x05]); // -3
        body.extend(one_byte("ratio"));
        body.push(b'N');
        body.extend(1.5_f64.to_le_bytes());
        body.extend(one_byte("at"));
        body.push(b'N');
        body.extend(1_795_531_375_809.0_f64.to_le_bytes());
        body.extend(one_byte("yes"));
        body.push(b'T');
        body.extend(one_byte("no"));
        body.push(b'F');
        body.extend(one_byte("none"));
        body.push(b'0');
        body.extend(one_byte("absent"));
        body.push(b'_');
        body.extend(one_byte("utf8"));
        body.extend([b'S', 2, b'o', b'k']);
        body.extend([b'{', 10]);

        assert_eq!(
            read(&v8(&body)).unwrap(),
            json!({
                "name": "Berlin",
                "russian": "Берлин",
                "count": 7,
                "negative": -3,
                "ratio": 1.5,
                "at": 1_795_531_375_809_i64,
                "yes": true,
                "no": false,
                "none": null,
                "absent": null,
                "utf8": "ok",
            })
        );
    }

    #[test]
    fn arrays_maps_sets_and_a_reference_to_an_object_read_before() {
        // { first: {id: 1}, again: <first>, list: [1, hole, 2], map: Map{"k": 1}, set: Set{2} }
        let mut body = vec![b'o'];
        body.extend(one_byte("first"));
        body.push(b'o');
        body.extend(one_byte("id"));
        body.extend([b'I', 2, b'{', 1]);
        body.extend(one_byte("again"));
        body.extend([b'^', 1]);
        body.extend(one_byte("list"));
        body.extend([b'A', 3, b'I', 2, b'-', b'I', 4, b'$', 0, 3]);
        body.extend(one_byte("map"));
        body.push(b';');
        body.extend(one_byte("k"));
        body.extend([b'I', 2, b':', 2]);
        body.extend(one_byte("set"));
        body.extend([b'\'', b'I', 4, b',', 1]);
        body.extend(one_byte("sparse"));
        body.extend([b'a', 3, b'I', 2, b'I', 8, b'@', 1, 3]);
        body.extend([b'{', 6]);

        assert_eq!(
            read(&v8(&body)).unwrap(),
            json!({
                "first": {"id": 1},
                "again": {"id": 1},
                "list": [1, null, 2],
                "map": {"k": 1},
                "set": [2],
                "sparse": [null, 4, null],
            })
        );
    }

    #[test]
    fn what_cannot_be_read_is_an_error_with_its_offset() {
        assert_eq!(read(&[0x0F]).unwrap_err().reason, "no V8 version header");
        let buffer = read(&v8(&[b'o', b'B', 0])).unwrap_err();
        assert_eq!(
            (buffer.reason.as_str(), buffer.offset),
            ("unknown tag 0x42", 3)
        );
        let cut = read(&v8(&[b'o', b'"', 9, b'a'])).unwrap_err();
        assert_eq!(cut.reason, "length past the end");
        let forward = read(&v8(&[b'^', 0])).unwrap_err();
        assert_eq!(forward.reason, "reference to an object not read");
        let deep: Vec<u8> = std::iter::repeat_n(b'A', 300)
            .flat_map(|tag| [tag, 1])
            .collect();
        assert_eq!(read(&v8(&deep)).unwrap_err().reason, "values nest too deep");
        let odd = read(&v8(&[b'c', 3, 0x41, 0, 0x42])).unwrap_err();
        assert_eq!(odd.reason, "two-byte string of odd length");
        let long_sparse = read(&v8(&[b'a', 0x81, 0x80, 0x04, b'@', 0, 0])).unwrap_err();
        assert_eq!(long_sparse.reason, "sparse array too long");
        let varint = read(&v8(&[
            b'U', 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 1,
        ]));
        assert_eq!(varint.unwrap_err().reason, "varint too long");
        let wide = read(&v8(&[b'U', 0x80, 0x80, 0x80, 0x80, 0x10])).unwrap_err();
        assert_eq!(wide.reason, "varint too large");
    }

    #[test]
    fn boxed_values_dates_and_counts_read_as_their_plain_value() {
        let mut body = vec![b'A', 9, b'?', 4, b'?', 0, b'U', 7, b'D'];
        body.extend(1_795_531_375_809.0_f64.to_le_bytes());
        body.push(b'n');
        body.extend(2.5_f64.to_le_bytes());
        body.extend(b"yxs");
        body.extend(one_byte("boxed"));
        body.push(b'R');
        body.extend(one_byte("a+"));
        body.push(1); // the flags of the expression
        // A boxed value is an object (the array is 0, the date 1): a reference reads it again.
        body.extend([b'^', 2, b'^', 5]);
        body.extend([b'$', 0, 9]);
        assert_eq!(
            read(&v8(&body)).unwrap(),
            json!([
                7,
                1_795_531_375_809_i64,
                2.5,
                true,
                false,
                "boxed",
                "a+",
                2.5,
                "boxed"
            ])
        );
    }

    #[test]
    fn counts_before_a_value_are_skipped_without_using_the_stack() {
        let mut body: Vec<u8> = std::iter::repeat_n([b'?', 0], 1_000_000)
            .flatten()
            .collect();
        body.push(b'T');
        assert_eq!(read(&v8(&body)).unwrap(), json!(true));
        let endless: Vec<u8> = std::iter::repeat_n([b'?', 0], 1_000_000)
            .flatten()
            .collect();
        assert_eq!(read(&v8(&endless)).unwrap_err().reason, "unexpected end");
    }

    #[test]
    fn a_small_input_that_stands_for_a_huge_value_is_refused() {
        // Each object holds the one before it twice: 40 objects would be 2^40 values.
        let mut body = vec![b'A', 41, b'o', b'{', 0];
        for id in 1..41_u8 {
            body.push(b'o');
            body.extend(one_byte("a"));
            body.extend([b'^', id]);
            body.extend(one_byte("b"));
            body.extend([b'^', id, b'{', 2]);
        }
        body.extend([b'$', 0, 41]);
        assert_eq!(read(&v8(&body)).unwrap_err().reason, "value too large");
        // 250 arrays, one inside the other, around a string of 1.1 MB: each array keeps a copy
        // of what it holds, for references to it.
        let mut body: Vec<u8> = std::iter::repeat_n([b'A', 1], 250).flatten().collect();
        body.extend([b'"', 0xE0, 0x91, 0x43]); // the length 1,100,000
        body.extend(std::iter::repeat_n(b'x', 1_100_000));
        body.extend(std::iter::repeat_n([b'$', 0, 1], 250).flatten());
        assert_eq!(read(&v8(&body)).unwrap_err().reason, "value too large");
        // 300 sparse arrays of the longest length.
        let mut body = vec![b'A', 0xAC, 0x02];
        for _ in 0..300 {
            body.extend([b'a', 0x80, 0x80, 0x04, b'@', 0, 0]);
        }
        body.extend([b'$', 0, 0]);
        assert_eq!(read(&v8(&body)).unwrap_err().reason, "value too large");
    }
}
