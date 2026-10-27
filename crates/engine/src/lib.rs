//! Evaluate Lua profiles into documents with hooks.

pub mod api;
pub mod assembly;
pub mod engine;
mod loader;
pub mod project;
pub mod stubs;

pub use assembly::assemble;
pub use engine::{Engine, Evaluated};

#[cfg(test)]
mod tests {
    #[test]
    fn face_fault_moves_straight_into_detail() {
        let fault = confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(1),
                confit_binding::fault::Segment::Field("path".to_owned()),
                confit_binding::fault::Segment::Field("base".to_owned()),
            ],
            reason: confit_binding::fault::FaultReason::UnknownVariant {
                found: "\"void\"".to_owned(),
                options: vec![
                    "home".to_owned(),
                    "config".to_owned(),
                    "data".to_owned(),
                    "cache".to_owned(),
                    "literal".to_owned(),
                ],
            },
        };
        assert_eq!(
            confit_core::error::EvalDetail::Face(fault.clone()),
            confit_core::error::EvalDetail::Face(confit_binding::fault::FaceFault {
                segments: vec![
                    confit_binding::fault::Segment::Field("documents".to_owned()),
                    confit_binding::fault::Segment::Index(1),
                    confit_binding::fault::Segment::Field("path".to_owned()),
                    confit_binding::fault::Segment::Field("base".to_owned()),
                ],
                reason: confit_binding::fault::FaultReason::UnknownVariant {
                    found: "\"void\"".to_owned(),
                    options: vec![
                        "home".to_owned(),
                        "config".to_owned(),
                        "data".to_owned(),
                        "cache".to_owned(),
                        "literal".to_owned(),
                    ],
                },
            })
        );
    }

    #[test]
    fn shape_mismatch_moves_straight_into_detail() {
        let fault = confit_binding::fault::FaceFault {
            segments: vec![confit_binding::fault::Segment::Index(2)],
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a string".to_owned(),
                found: "integer".to_owned(),
            },
        };
        assert_eq!(
            confit_core::error::EvalDetail::Face(fault.clone()),
            confit_core::error::EvalDetail::Face(confit_binding::fault::FaceFault {
                segments: vec![confit_binding::fault::Segment::Index(2)],
                reason: confit_binding::fault::FaultReason::ShapeMismatch {
                    require: "a string".to_owned(),
                    found: "integer".to_owned(),
                },
            })
        );
    }
}
