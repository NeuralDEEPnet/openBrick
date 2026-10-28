//! The `credit` block every data document carries (LICENSE-POLICY.md): the
//! standard, process and fit profiles, and parts. It lives in this crate because
//! the Dimensional Standard is the first document that needs it and every other
//! document crate depends on this one.

/// Who made a document. Keeping it is a community norm, not a legal condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Credit {
    pub author: String,
    /// Home page of the author; omitted from written documents when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}
