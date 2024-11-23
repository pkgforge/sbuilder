use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter, Write},
};

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize,
};

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum DistroPkg {
    List(Vec<String>),
    InnerNode(HashMap<String, DistroPkg>),
}

#[derive(Debug)]
struct DistroPkgVisitor;

impl<'de> Visitor<'de> for DistroPkgVisitor {
    type Value = DistroPkg;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a map or a list")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut vec = Vec::new();
        while let Some(value) = seq.next_element()? {
            vec.push(value);
        }
        Ok(DistroPkg::List(vec))
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let node: HashMap<String, DistroPkg> =
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(DistroPkg::InnerNode(node))
    }
}

impl<'de> Deserialize<'de> for DistroPkg {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DistroPkgVisitor)
    }
}

impl DistroPkg {
    pub fn write_yaml(&self, writer: &mut BufWriter<File>, indent: usize) -> io::Result<()> {
        let indent_str = " ".repeat(indent);

        match self {
            DistroPkg::List(items) => {
                for item in items {
                    writeln!(writer, "{}  - \"{}\"", indent_str, item)?;
                }
            }
            DistroPkg::InnerNode(map) => {
                for (key, value) in map {
                    writeln!(writer, "{}  {}:", indent_str, key)?;
                    value.write_yaml(writer, indent + 2)?;
                }
            }
        }
        Ok(())
    }
}
