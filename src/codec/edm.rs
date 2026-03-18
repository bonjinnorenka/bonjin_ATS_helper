use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdmType {
    String,
    Boolean,
    Int32,
    Int64,
    Double,
    Guid,
    Binary,
    DateTime,
}

impl EdmType {
    pub(crate) fn from_annotation(value: &str) -> Option<Self> {
        match value {
            "Edm.String" => Some(Self::String),
            "Edm.Boolean" => Some(Self::Boolean),
            "Edm.Int32" => Some(Self::Int32),
            "Edm.Int64" => Some(Self::Int64),
            "Edm.Double" => Some(Self::Double),
            "Edm.Guid" => Some(Self::Guid),
            "Edm.Binary" => Some(Self::Binary),
            "Edm.DateTime" => Some(Self::DateTime),
            _ => None,
        }
    }

    pub(crate) fn annotation(self) -> Option<&'static str> {
        match self {
            Self::String | Self::Boolean | Self::Int32 | Self::Double => None,
            Self::Int64 => Some("Edm.Int64"),
            Self::Guid => Some("Edm.Guid"),
            Self::Binary => Some("Edm.Binary"),
            Self::DateTime => Some("Edm.DateTime"),
        }
    }
}

impl Display for EdmType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::String => "Edm.String",
            Self::Boolean => "Edm.Boolean",
            Self::Int32 => "Edm.Int32",
            Self::Int64 => "Edm.Int64",
            Self::Double => "Edm.Double",
            Self::Guid => "Edm.Guid",
            Self::Binary => "Edm.Binary",
            Self::DateTime => "Edm.DateTime",
        };
        f.write_str(value)
    }
}
