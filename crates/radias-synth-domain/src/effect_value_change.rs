//! Complete value/header edit chains of SYS08017A and SYS08025C.
use crate::{
    effect_header_event::{EffectHeaderChange, EffectHeaderEvent},
    effect_parameter_caller::{EffectParameterCallerTables, PreparedEffectParameterCaller},
    effect_property::{EffectProperty, EffectPropertyChange, EffectPropertyTables},
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlTables},
    insert_parameter_caller::InsertParameterChange,
    master_initial_mask::MasterInitialMaskContext,
    mixed_effect_parameter::EffectParameterTarget,
};
pub struct PreparedEffectValueChange {
    pub call: PreparedEffectParameterCaller,
    pub value: i32,
    pub event_dispatched: bool,
}
impl InsertControlTables {
    pub fn prepare_effect_value_change(
        &self,
        state: &InsertControlState,
        properties: &EffectPropertyTables,
        callers: &EffectParameterCallerTables,
        edit: EffectPropertyChange,
        context: MasterInitialMaskContext<'_>,
    ) -> Option<PreparedEffectValueChange> {
        if edit.property == EffectProperty::Kind {
            return None;
        }
        let old = properties.displayed_value(context.common.program, edit.target, edit.property)?;
        let p = properties.prepare(state, context.common.program, edit)?;
        // SYS07570A classifies Insert enable as event0; SYS08025C skips
        // that callback even if the stored enable bit changed. Master calls
        // its event entry unconditionally.
        let dispatch = edit.target == EffectParameterTarget::Master
            || (edit.property != EffectProperty::Enabled && old != p.value);
        let call = if dispatch {
            let common = InsertControlContext {
                program: &p.program,
                ..context.common
            };
            match edit.property {
                EffectProperty::Parameter(parameter) => match edit.target {
                    EffectParameterTarget::Master => self.prepare_master_parameter_caller(
                        &p.next,
                        parameter,
                        MasterInitialMaskContext { common, ..context },
                        callers,
                    )?,
                    EffectParameterTarget::Insert(slot) => self.prepare_insert_parameter_caller(
                        &p.next,
                        InsertParameterChange { slot, parameter },
                        common,
                        callers,
                    )?,
                },
                EffectProperty::Enabled | EffectProperty::Owner(_) => self
                    .prepare_effect_header_event(
                        &p.next,
                        &p.program,
                        EffectHeaderChange {
                            target: edit.target,
                            event: match edit.property {
                                EffectProperty::Enabled => EffectHeaderEvent::Enabled,
                                EffectProperty::Owner(n) => EffectHeaderEvent::Owner(n),
                                _ => return None,
                            },
                        },
                        context.common.midi.direct_switch,
                    )?,
                EffectProperty::Kind => return None,
            }
        } else {
            PreparedEffectParameterCaller {
                next: p.next,
                program: p.program,
                steps: [None; 2],
                step_count: 0,
            }
        };
        Some(PreparedEffectValueChange {
            call,
            value: p.value,
            event_dispatched: dispatch,
        })
    }
}
