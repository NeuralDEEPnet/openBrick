//! `schemas/part.schema.json` and this crate must agree on the feature ops: the
//! schema's op enum is exactly `KNOWN_OPS`. (`cargo xtask data` checks the rest:
//! every valid part fixture and every file in parts/ loads with `PartDoc::parse`,
//! and what it writes back passes the schema.)

use openbrick_part::KNOWN_OPS;

#[test]
fn schema_op_enum_equals_known_ops() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schemas/part.schema.json"))
            .expect("schemas/part.schema.json is JSON");
    let ops: Vec<&str> = schema
        .pointer("/$defs/feature/properties/op/enum")
        .and_then(|v| v.as_array())
        .expect("op enum at /$defs/feature/properties/op/enum")
        .iter()
        .map(|v| v.as_str().expect("every op is a string"))
        .collect();
    assert_eq!(
        ops, KNOWN_OPS,
        "the op enum in schemas/part.schema.json and openbrick_part::KNOWN_OPS list different ops; \
         add or remove the op in both places, in the same order"
    );
}
