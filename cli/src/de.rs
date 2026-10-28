//! the parsed JSON tree as a serde data format, read-only as the crate writes its one
//! JSON body by hand: [`Deserializer`] reads any `Deserialize` type from a [`Value`],
//! and a failed read is an [`Error`], never a panic; see `docs/operations/cli.md`.

use std::fmt;

use serde::de::value::BorrowedStrDeserializer;
use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, Unexpected,
    VariantAccess, Visitor,
};

use crate::fold;
use crate::json::{Number, Object, Value};

/// the newtype struct name that asks this reader for a [`Value`], with a number
/// delivered as its literal text.
const VALUE_TOKEN: &str = "$marshal::Value";

/// why a value could not be read into the requested type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

/// reads a type from a parsed value.
#[derive(Debug, Clone, Copy)]
pub struct Deserializer<'de> {
    value: &'de Value,
    ignore_field_case: bool,
}

impl<'de> Deserializer<'de> {
    #[must_use]
    pub fn new(value: &'de Value) -> Self {
        Self {
            value,
            ignore_field_case: false,
        }
    }

    /// matches member names to struct field names ignoring ASCII case, as PowerShell
    /// does; map keys and enum variants stay exact.
    #[must_use]
    pub fn ignoring_field_case(self) -> Self {
        Self {
            ignore_field_case: true,
            ..self
        }
    }

    fn within(self, value: &'de Value) -> Self {
        Self { value, ..self }
    }

    fn unexpected(&self) -> Unexpected<'de> {
        match self.value {
            Value::Null => Unexpected::Unit,
            Value::Bool(flag) => Unexpected::Bool(*flag),
            Value::Number(_) => Unexpected::Other("number"),
            Value::String(text) => Unexpected::Str(text),
            Value::Array(_) => Unexpected::Seq,
            Value::Object(_) => Unexpected::Map,
        }
    }

    fn invalid_type(&self, expected: &dyn de::Expected) -> Error {
        de::Error::invalid_type(self.unexpected(), expected)
    }
}

/// hands a number to the visitor as the narrowest integer it fits, else a double.
fn visit_number<'de, V: Visitor<'de>>(number: &Number, visitor: V) -> Result<V::Value, Error> {
    match number.as_integer() {
        Some(integer) => {
            if let Ok(unsigned) = u64::try_from(integer) {
                visitor.visit_u64(unsigned)
            } else if let Ok(signed) = i64::try_from(integer) {
                visitor.visit_i64(signed)
            } else {
                visitor.visit_i128(integer)
            }
        }
        None => visitor.visit_f64(number.as_f64()),
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(flag) => visitor.visit_bool(*flag),
            Value::Number(number) => visit_number(number, visitor),
            Value::String(text) => visitor.visit_borrowed_str(text),
            Value::Array(items) => visitor.visit_seq(Elements {
                items: items.iter(),
                reader: self,
            }),
            Value::Object(object) => visitor.visit_map(Members::new(object.iter(), self, None)),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::Number(number) if name == VALUE_TOKEN => visitor
                .visit_newtype_struct(BorrowedStrDeserializer::<Error>::new(number.literal())),
            _ if name == VALUE_TOKEN => self.deserialize_any(visitor),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::Object(object) => {
                let folding = self.ignore_field_case.then_some(fields);
                visitor.visit_map(Members::new(object.iter(), self, folding))
            }
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::String(variant) => {
                visitor.visit_enum(BorrowedStrDeserializer::<Error>::new(variant))
            }
            Value::Object(object) => {
                let mut members = object.iter();
                match (members.next(), members.next()) {
                    (Some((variant, payload)), None) => visitor.visit_enum(Variant {
                        name: variant,
                        payload,
                        reader: self,
                    }),
                    _ => Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"a map with a single key naming the variant",
                    )),
                }
            }
            _ => Err(self.invalid_type(&visitor)),
        }
    }

    fn is_human_readable(&self) -> bool {
        true
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map identifier
        ignored_any
    }
}

struct Elements<'de> {
    items: std::slice::Iter<'de, Value>,
    reader: Deserializer<'de>,
}

impl<'de> SeqAccess<'de> for Elements<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.items.next() {
            Some(item) => seed.deserialize(self.reader.within(item)).map(Some),
            None => Ok(None),
        }
    }
}

/// the members of an object, in document order; `folding` lists the field names a
/// struct read matches member names to, when the caller asked to ignore case.
struct Members<'de, I> {
    members: I,
    next_value: Option<&'de Value>,
    reader: Deserializer<'de>,
    folding: Option<&'static [&'static str]>,
}

impl<'de, I> Members<'de, I> {
    fn new(
        members: I,
        reader: Deserializer<'de>,
        folding: Option<&'static [&'static str]>,
    ) -> Self {
        Self {
            members,
            next_value: None,
            reader,
            folding,
        }
    }
}

impl<'de, I> MapAccess<'de> for Members<'de, I>
where
    I: Iterator<Item = (&'de str, &'de Value)>,
{
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((name, value)) = self.members.next() else {
            return Ok(None);
        };
        self.next_value = Some(value);
        let name = self
            .folding
            .and_then(|fields| fields.iter().find(|field| fold::eq_ascii(field, name)))
            .copied()
            .unwrap_or(name);
        seed.deserialize(BorrowedStrDeserializer::new(name))
            .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        match self.next_value.take() {
            Some(value) => seed.deserialize(self.reader.within(value)),
            None => Err(de::Error::custom(
                "a member value was requested before its name",
            )),
        }
    }
}

/// an enum read from an object with one member, whose name is the variant and whose
/// value is its content.
struct Variant<'de> {
    name: &'de str,
    payload: &'de Value,
    reader: Deserializer<'de>,
}

impl<'de> EnumAccess<'de> for Variant<'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let variant = seed.deserialize(BorrowedStrDeserializer::new(self.name))?;
        Ok((variant, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.payload {
            Value::Null => Ok(()),
            _ => Err(self
                .reader
                .within(self.payload)
                .invalid_type(&"a unit variant")),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self.reader.within(self.payload))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_seq(self.reader.within(self.payload), visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_struct(self.reader.within(self.payload), "", fields, visitor)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct(VALUE_TOKEN, ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a value of the parsed JSON tree")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, flag: bool) -> Result<Value, E> {
        Ok(Value::Bool(flag))
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Value, E> {
        Ok(Value::String(text.to_string()))
    }

    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        String::deserialize(deserializer)
            .map(|literal| Value::Number(Number::from_literal(literal)))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Object::default();
        while let Some((name, value)) = map.next_entry::<String, Value>()? {
            object.insert(name, value).map_err(de::Error::custom)?;
        }
        Ok(Value::Object(object))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Deserialize;

    use super::*;
    use crate::json;

    fn tree(text: &str) -> Value {
        json::parse(text.as_bytes()).unwrap()
    }

    fn read<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
        T::deserialize(Deserializer::new(&tree(text)))
    }

    fn read_ignoring_case<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
        T::deserialize(Deserializer::new(&tree(text)).ignoring_field_case())
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Sample {
        name: String,
        tags: Vec<String>,
        limits: BTreeMap<String, u32>,
        note: Option<String>,
        kind: Kind,
        token_path: Option<String>,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    enum Kind {
        Plain,
        Sized(u32),
        Pair(u8, bool),
        Named { label: String },
    }

    #[test]
    fn reads_a_struct_list_map_option_enum_and_ignored_member() {
        let sample: Sample = read(
            r#"{
                "name": "pool",
                "ignored": { "deep": [1, 2.5, { "x": null }], "text": "y" },
                "tags": ["a", "b"],
                "limits": { "cpus": 2, "memory": 8 },
                "note": null,
                "kind": { "Named": { "label": "l" } },
                "tokenPath": "/t"
            }"#,
        )
        .unwrap();
        assert_eq!(
            sample,
            Sample {
                name: "pool".into(),
                tags: vec!["a".into(), "b".into()],
                limits: BTreeMap::from([("cpus".into(), 2), ("memory".into(), 8)]),
                note: None,
                kind: Kind::Named { label: "l".into() },
                token_path: Some("/t".into()),
            }
        );
    }

    #[test]
    fn reads_every_enum_form() {
        assert_eq!(read::<Kind>(r#""Plain""#).unwrap(), Kind::Plain);
        assert_eq!(read::<Kind>(r#"{ "Plain": null }"#).unwrap(), Kind::Plain);
        assert_eq!(read::<Kind>(r#"{ "Sized": 3 }"#).unwrap(), Kind::Sized(3));
        assert_eq!(
            read::<Kind>(r#"{ "Pair": [1, true] }"#).unwrap(),
            Kind::Pair(1, true)
        );
        assert!(read::<Kind>(r#"{ "Plain": 1 }"#).is_err());
        assert!(read::<Kind>(r#"{ "Plain": null, "Sized": 3 }"#).is_err());
        assert!(read::<Kind>(r#""Unknown""#).is_err());
        assert!(read::<Kind>("3").is_err());
    }

    #[test]
    fn a_serde_type_error_comes_back_as_an_error() {
        let error = read::<Sample>(r#"{ "name": 5 }"#).unwrap_err();
        assert!(error.to_string().contains("invalid type"), "{error}");
        let error = read::<u8>("300").unwrap_err();
        assert!(error.to_string().contains("300"), "{error}");
        let error = read::<Sample>(r#"{ "name": "n" }"#).unwrap_err();
        assert!(error.to_string().contains("missing field"), "{error}");
        assert!(read::<Vec<String>>(r#"["a", 1]"#).is_err());
    }

    #[test]
    fn a_struct_read_from_a_node_that_is_not_an_object_is_an_error() {
        for text in ["null", "true", "3", r#""text""#, "[]", r#"["a", [], 3]"#] {
            let error = read::<Sample>(text).unwrap_err();
            assert!(
                error.to_string().contains("invalid type"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn a_foreign_target_sees_integers_and_doubles() {
        assert_eq!(read::<i64>("-5").unwrap(), -5);
        assert_eq!(read::<u64>("18446744073709551615").unwrap(), u64::MAX);
        assert_eq!(read::<i64>("-9223372036854775808").unwrap(), i64::MIN);
        assert_eq!(read::<i128>(&i128::MAX.to_string()).unwrap(), i128::MAX);
        assert_eq!(read::<i128>(&i128::MIN.to_string()).unwrap(), i128::MIN);
        assert_eq!(read::<f64>("1.5").unwrap(), 1.5);
        assert_eq!(read::<f64>("8").unwrap(), 8.0);
        assert!(read::<u8>("8.0").is_err());
        assert!(read::<i64>("1e1").is_err());
        assert!(read::<i64>("18446744073709551616").is_err());
        assert_eq!(read::<f64>("1e400").unwrap(), f64::INFINITY);
        assert_eq!(read::<f64>("-1e400").unwrap(), f64::NEG_INFINITY);
    }

    #[test]
    fn the_trees_own_value_keeps_the_literal_text() {
        let text = "[1e400, 8.0, -0, 123456789012345678901234567890123456789012345678901234567890, \
                    {\"a\": [null, true, \"s\"], \"b\": {}}]";
        let original = tree(text);
        let copy = Value::deserialize(Deserializer::new(&original)).unwrap();
        assert_eq!(copy, original);
        let Value::Array(items) = copy else {
            panic!("not a list");
        };
        assert!(matches!(items.first(), Some(Value::Number(n)) if n.literal() == "1e400"));
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields, rename_all = "camelCase")]
    struct Strict {
        owner: String,
        memory_gb: Option<u32>,
        #[serde(default)]
        nested: Option<Box<Strict>>,
    }

    #[test]
    fn field_names_match_exactly_unless_the_caller_opts_in() {
        let text = r#"{ "Owner": "o", "MEMORYGB": 4 }"#;
        let error = read::<Strict>(text).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
        let strict: Strict = read_ignoring_case(text).unwrap();
        assert_eq!(strict.owner, "o");
        assert_eq!(strict.memory_gb, Some(4));
    }

    #[test]
    fn opting_in_to_case_folding_does_not_make_an_unknown_field_known() {
        let error = read_ignoring_case::<Strict>(r#"{ "owner": "o", "other": 1 }"#).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[test]
    fn case_folding_applies_to_struct_fields_and_not_to_map_keys_or_variants() {
        let limits: BTreeMap<String, u32> = read_ignoring_case(r#"{ "Cpus": 2 }"#).unwrap();
        assert_eq!(limits.keys().collect::<Vec<_>>(), ["Cpus"]);
        assert!(read_ignoring_case::<Kind>(r#""plain""#).is_err());
        let nested: Strict =
            read_ignoring_case(r#"{ "OWNER": "o", "nested": { "Owner": "p" } }"#).unwrap();
        assert_eq!(nested.nested.map(|inner| inner.owner).as_deref(), Some("p"));
    }
}
