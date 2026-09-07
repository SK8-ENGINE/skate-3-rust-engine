//! Original behavior lifecycle dispatch for the persistent MotionHost.
use super::*;
use crate::graph_host::motion_kickturn::{self, Operation as KickTurnOperation};

impl MotionHost {
    pub(super) fn execute(
        &mut self,
        behavior: BehaviorId,
        frame: &Frame,
        phase: u8,
    ) -> Result<(), String> {
        let id = *self
            .remap
            .behaviors
            .get(behavior)
            .ok_or("Unbound MotionGraph behavior")?;
        let operation = self.operations[id].clone();
        if let MotionOperation::StockGameplay(operation) = operation {
            return super::stock_execution::execute(self, operation, phase);
        }
        if let MotionOperation::EndGesture = operation {
            if phase == 0 { self.end_gesture_channels(); }
            return Ok(());
        }
        if let MotionOperation::Grind(operation) = operation {
            let Instance::Grind(state)=&mut self.instances[behavior] else {return Err("Grind instance mismatch".into())};
            let height=self.crouching_physical.map(|p|p.animation_height_72).ok_or("Grind requires physical height")?;
            return super::super::motion_grind::execute(state,&operation,phase,frame.dt,
                &self.grind_settings,&self.grind_physical,height,&mut self.animation);
        }
        if let MotionOperation::OffboardAir(operation) = operation {
            let Instance::OffboardAir(state) = &mut self.instances[behavior] else {
                return Err("OffboardAir instance mismatch".into());
            };
            state.execute(
                &operation,
                phase,
                self.offboard_output,
                &self.action_intents,
                &mut self.animation,
            )?;
            if let Some(value) = state.seed_write {
                self.wipeout_controls.seed_from_air_tweak = value;
            }
            return Ok(());
        }
        if let MotionOperation::ToggleBoard = operation {
            let Instance::ToggleBoard(state) = &mut self.instances[behavior] else {
                return Err("ToggleBoard instance mismatch".into());
            };
            return state.execute(
                phase,
                self.toggle_board_physical,
                &self.action_intents,
                &mut self.animation,
            );
        }
        if let MotionOperation::Cadence(operation) = operation {
            use crate::graph_host::motion_cadence::{Operation, biped_cadence};
            match operation {
                Operation::BipedCadence if phase == 1 => {
                    let mut value = self.offboard_phase;
                    biped_cadence(Some(self.offboard_phase), Some(&mut value), phase);
                    self.phase_write = Some(value);
                }
                Operation::MatchCadence => {
                    let Instance::Cadence(state) = &mut self.instances[behavior] else {
                        return Err("Cadence operation/instance mismatch".into());
                    };
                    match phase {
                        0 => state.begin(Some(self.offboard_phase), true),
                        1 => state.update(Some(&mut self.animation)),
                        _ => state.end(),
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if let MotionOperation::Trick(operation) = operation {
            let Some(&Instance::Trick(mut updates)) = self.instances.get(behavior) else {
                return Err("Trick operation/instance mismatch".into());
            };
            let result = super::super::motion_tricks::execute(self, operation, &mut updates, phase);
            self.instances[behavior] = Instance::Trick(updates);
            return result;
        }
        if let MotionOperation::Slide(operation) = operation {
            return self.execute_slide(behavior, operation, frame, phase);
        }
        if matches!(
            operation,
            MotionOperation::ClearTrickAttr
                | MotionOperation::AirLeg(_)
                | MotionOperation::BodySpin
        ) {
            return self.execute_air(behavior, operation, frame, phase);
        }
        if let MotionOperation::TwistLean(operation) = operation {
            return self.execute_twist_lean(behavior, operation, phase);
        }
        if let MotionOperation::Wipeout(operation) = operation {
            return self.execute_wipeout(behavior, operation, phase);
        }
        if let MotionOperation::IntentFilter(operation) = operation {
            return self.execute_intent_filter(behavior, operation, frame, phase);
        }
        if let MotionOperation::ResetAnimation(operation) = operation {
            if phase == 0 {
                match operation {
                    crate::graph_host::motion_reset::Operation::SkaterAnimation => {
                        self.action_intents.clear();
                        self.animation.motion_intents.clear();
                        self.animation.filtered_intents.clear();
                        self.animation.reset_from_stock();
                    }
                    crate::graph_host::motion_reset::Operation::GivenStance => {
                        self.animation.posture.set_requested(false);
                    }
                }
            }
            return Ok(());
        }
        if let MotionOperation::HandService(operation) = operation {
            self.hand_services
                .execute(operation, phase, &mut self.animation);
            return Ok(());
        }
        if let MotionOperation::Landing(operation) = &operation {
            let Instance::Landing(state) = self
                .instances
                .get_mut(behavior)
                .ok_or("Unallocated landing behavior")?
            else {
                return Err("Landing operation/instance mismatch".into());
            };
            //StoreLandingData can update C54 earlier in this same graph traversal.
            let physical = self.landing_physical.map(|mut p| {
                p.last_good_landing_velocity = self.riding.last_good_landing_velocity;
                p
            });
            return super::super::motion_landing_execute::execute(
                operation,
                state,
                &mut self.flags,
                &mut self.animation,
                &self.condition_random,
                physical,
                self.playback_context.is_mirrored,
                frame.dt,
                phase,
            );
        }
        let instance = self
            .instances
            .get_mut(behavior)
            .ok_or("Unallocated MotionGraph behavior")?;
        match (operation, instance) {
            (MotionOperation::AddRunoutAttribs, Instance::Runout(state)) => {
                if phase == 0 {
                    *state = Some(super::super::motion_runout::capture(
                        self.runout_physical
                            .ok_or("AddRunoutAttribs requires completed physical output")?,
                        self.animation
                            .skater_animation_flags
                            .ok_or("AddRunoutAttribs requires animation stance")?
                            & 0x4000_0000
                            != 0,
                    ));
                } else if phase == 1 {
                    let values = state.ok_or("AddRunoutAttribs updated before Begin")?;
                    for (name, value) in [
                        ("BipedStartAngle", values.angle_degrees),
                        ("BipedSpeed", values.speed),
                    ] {
                        self.animation.set_attribute(SettableAttribute {
                            name: encode(name.as_bytes()),
                            value,
                            normalized: false,
                            sequence_id: -1,
                        });
                    }
                }
            }
            //Retail ctor82BA58E8 retains only diagnostic text/layout. All three
            //lifecycle slots in82309664 are82B61BB8 (blr), with no state writes.
            (MotionOperation::PrintText2D, _) => {}
            (MotionOperation::SetBumpCoefficients(names), _) => {
                // Begin only: vtable823200C8+48; Update/End are82B61BB8.
                if phase == 0 {
                    use skate_core::animation::playback_parameters::{
                        AttributeSink, SettableAttribute,
                    };
                    let acceleration = self
                        .bump_acceleration
                        .ok_or("SetBumpCoefficients requires PhysOutAnimation112")?;
                    let flags = self
                        .animation
                        .skater_animation_flags
                        .ok_or("SetBumpCoefficients requires live ISkaterAnim stance")?;
                    let values = skate_core::animation::bump::coefficients(
                        acceleration,
                        flags & 0x4000_0000 != 0,
                        &self.bump_settings,
                    );
                    for (name, value) in names.into_iter().zip(values) {
                        AttributeSink::set_attribute(
                            &mut self.animation,
                            SettableAttribute {
                                name,
                                value,
                                normalized: false,
                                sequence_id: -1,
                            },
                        );
                    }
                }
            }
            (MotionOperation::Shove(operation), Instance::Shove(state)) => match phase {
                0 => state.begin(),
                1 => state.update(
                    &operation,
                    &mut self.animation,
                    self.shove_physical
                        .ok_or("Shove requires actual interaction output")?,
                    self.hand_services.busy_hands,
                    self.playback_context
                        .is_mirrored
                        .ok_or("Shove requires animation stance")?,
                )?,
                _ => state.end(&mut self.animation, self.hand_services.keep_shove_channels),
            },
            (MotionOperation::CharacterGesture, Instance::CharacterGesture(state)) => {
                if phase == 1 {
                    let physical = self.gesture_physical.ok_or(
                        "CharacterGesture requires original physical and hostgesture publication",
                    )?;
                    let gesture_intents = self.animation.motion_intents.clone();
                    let inputs =
                        crate::graph_host::motion_character_gesture::CharacterGestureInputs {
                            motion_intents: &gesture_intents,
                            busy_hands: self.hand_services.busy_hands,
                            filtered_state: self
                                .condition_inputs
                                .physical_state
                                .as_ref()
                                .ok_or("CharacterGesture requires filteredcategory")?
                                .category,
                            ground321: physical.ground321,
                            board_held311: self
                                .playback_context
                                .board_available
                                .ok_or("CharacterGesture requires boardheldbyte311")?,
                            state_offboard75: physical.state_offboard75,
                            distance_to_cog: self
                                .crouching_physical
                                .ok_or("CharacterGesture requires actualheight")?
                                .animation_height_72,
                            selections: physical.selections,
                            suppress_up: physical.suppress_up,
                            force_brake_bypass: physical.force_brake_bypass,
                        };
                    if let Some(output) = state.update(&mut self.animation, inputs)? {
                        self.gesture_publication = Some(output);
                    }
                }
            }
            (
                MotionOperation::Native(
                    crate::graph_host::motion_native::Operation::StoreLandingData,
                ),
                Instance::LandingData(state),
            ) => {
                if phase == 1 {
                    state.update(
                        self.condition_inputs
                            .physical_state
                            .as_ref()
                            .ok_or("StoreLandingData requires physical category")?
                            .category,
                        self.native_physical
                            .ok_or("StoreLandingData requires raw physical COM velocity/up")?,
                        &mut self.riding,
                    );
                }
            }
            (
                MotionOperation::Native(crate::graph_host::motion_native::Operation::EndShimmy(
                    time,
                )),
                _,
            ) => {
                if phase == 0 {
                    crate::graph_host::motion_native::end_shimmy(&mut self.animation, time);
                }
            }
            (
                MotionOperation::Native(crate::graph_host::motion_native::Operation::Score {
                    regular,
                    mirrored,
                }),
                _,
            ) => {
                if phase == 1 {
                    self.score_packet.set(
                        if self
                            .playback_context
                            .is_mirrored
                            .ok_or("Score augmentation requires animation stance")?
                        {
                            mirrored
                        } else {
                            regular
                        },
                    );
                }
            }
            (MotionOperation::KickTurn(KickTurnOperation::ResetTimer), _) => {
                //82BAE8D0 is both Begin and End; v136=8258F930 clears3224.
                if phase == 0 || phase == 2 {
                    self.riding.time_since_kickturn = 0.0;
                }
            }
            (
                MotionOperation::KickTurn(KickTurnOperation::Steering(parameters)),
                Instance::KickTurn(state),
            ) => {
                motion_kickturn::execute(
                    state,
                    parameters,
                    &self.kickturn,
                    &mut self.animation,
                    frame.dt,
                    phase,
                )?;
            }
            (MotionOperation::FakieHeadChannel, Instance::FakieHead(state)) => {
                if phase == 1 {
                    let fakie = self
                        .animation
                        .skater_animation_flags
                        .ok_or("FakieHeadChannel requires actual animation flags")?
                        & 0x20000000
                        != 0;
                    state.update(
                        &mut self.animation,
                        self.flags.manualing,
                        self.is_power_sliding,
                        fakie,
                    )?;
                }
            }
            (MotionOperation::Pumping, Instance::Pumping(state)) => {
                const NAMES: [&str; 5] = ["PUMP0", "PUMP1", "PUMP2", "PUMP3", "PUMP4"]; //82F883C8
                if phase == 2 {
                    for name in NAMES {
                        self.animation.channels.end(name);
                    }
                } else if phase == 1 && self.allow_pumping {
                    let occupied = std::array::from_fn(|i| self.animation.channels.has(NAMES[i]));
                    let update = state.update(
                        self.pumping_acceleration
                            .ok_or("Pumping requires actual ground pumping acceleration")?,
                        occupied,
                        &self.pumping,
                    );
                    if let Some(index) = update.start {
                        let settings = skate_core::animation::channel_playback::ChannelSettings {
                            priority: 0,
                            keep_alive: false,
                            mirrored: false,
                            speed: 1.0,
                            blend_in: self.pumping.blend_in,
                            hold_during_blend_in: false,
                            blend_out: self.pumping.blend_out,
                            hold_during_blend_out: true,
                            use_attributes: false,
                        };
                        self.animation
                            .new_channel(NAMES[index], "B_PUMP", settings)?;
                    }
                    if let Some((index, value)) = update.influence {
                        self.animation.channels.influence(NAMES[index], value);
                    }
                }
            }
            (MotionOperation::DisallowPumping, _) => {
                if phase == 0 {
                    for name in ["PUMP0", "PUMP1", "PUMP2", "PUMP3", "PUMP4"] {
                        self.animation
                            .channels
                            .end_with(name, f32::from_bits(0x3dcccccd), false);
                    }
                    self.allow_pumping = false;
                } else if phase == 2 {
                    self.allow_pumping = true;
                }
            }
            (MotionOperation::SetSpeed { name, value }, _) => {
                if phase == 1 {
                    //82BB0948 sets a tree parameter; it does not call SetSpeed
                    //on the animation clock. Source uses abs(PhysOutMotion164).
                    let value = match value {
                        Some(value) => value,
                        None => {
                            self.crouching_physical
                                .ok_or("SetSpeed requires actual ground-projected speed")?
                                .body_164
                        }
                    };
                    self.animation.set_attribute(SettableAttribute {
                        name,
                        value: value.abs(),
                        normalized: false,
                        sequence_id: -1,
                    });
                }
            }
            (MotionOperation::UpdateRidingFakie(settings), Instance::RidingFakie(state)) => {
                if phase == 1 {
                    let mut physical = self
                        .fakie_physical
                        .ok_or("UpdateRidingFakie requires actual physical outputs")?;
                    physical.doing_trick = self.flags.doing_trick;
                    if let Some(fakie) = state.update(physical, frame.dt, settings) {
                        let flags = self
                            .animation
                            .skater_animation_flags
                            .as_mut()
                            .ok_or("UpdateRidingFakie requires actual animation flags")?;
                        *flags = (*flags & !0x20000000) | if fakie { 0x20000000 } else { 0 };
                    }
                }
            }
            (MotionOperation::SettingBodyTilt(name), Instance::BodyTilt(instance)) => {
                if phase == 1 && self.applying_body_tilt {
                    if let Some(value) = instance.update(
                        true,
                        self.playback_context
                            .is_mirrored
                            .ok_or("SettingBodyTilt needs actual mirrored stance")?,
                        self.body_tilt_physical
                            .ok_or("SettingBodyTilt needs actual PhysOut tilt and spin")?,
                        &self.body_tilt,
                    ) {
                        self.animation.set_attribute(SettableAttribute {
                            name,
                            value,
                            normalized: false,
                            sequence_id: -1,
                        });
                    }
                } else if phase == 1 {
                    // Disabled native branch updates the enable latch without
                    // reading physics or emitting a parameter.
                    instance.disable();
                }
            }
            (MotionOperation::Play(operation), Instance::Play(instance)) => match phase {
                0 => instance.begin(&operation, &mut self.playback_context, &mut self.animation)?,
                1 => instance.update(&operation, &mut self.animation)?,
                _ => instance.end(),
            },
            (
                MotionOperation::AttachIntent {
                    intent,
                    attribute,
                    set,
                },
                _,
            ) => {
                if phase == 1 {
                    self.animation.attach(&intent, attribute, set);
                }
            }
            (MotionOperation::Riding(operation), _) => self.riding.execute(
                operation,
                phase,
                frame,
                &mut self.animation,
                self.condition_inputs
                    .physical_state
                    .as_ref()
                    .map(|p| p.category),
                self.crouching_physical.map(|p| p.animation_height_72),
                self.flags.manualing,
            )?,
            (MotionOperation::ApplyingBodyTilt, _) => {
                if phase == 0 {
                    self.applying_body_tilt = true;
                } else if phase == 2 {
                    self.applying_body_tilt = false;
                }
            }
            (MotionOperation::Crouching(name), Instance::Crouching(state)) => {
                use skate_core::animation::crouching;
                if phase == 0 {
                    *state = Some(crouching::State::begin(
                        self.crouching_physical
                            .ok_or("Crouching Begin requires real physical height and speed")?,
                        &self.crouching,
                    ));
                } else if phase == 1 {
                    let intents = crouching::Intents {
                        auto_pump_angle: self.animation.motion_intent("AutoPumpAngle"),
                        auto_pump_magnitude: self.animation.motion_intent("AutoPumpMag"),
                        crouch: self.animation.motion_intent("Crouch"),
                        hard_turn_crouch: self.animation.motion_intent("HardTurnCrouch"),
                        manual: self.animation.motion_intent("Manual"),
                        motion_flag_108: self.is_power_sliding,
                    };
                    let result = state
                        .as_mut()
                        .ok_or("Crouching updated before Begin")?
                        .update(
                            self.crouching_physical
                                .ok_or("Crouching requires actual physical outputs")?,
                            intents,
                            frame.dt,
                            &self.crouching,
                        );
                    if result.new_auto_pump {
                        self.animation.emit_packet(encode(b"NewAutoPump"), 1.0);
                    }
                    if result.player_controlled_pump {
                        self.animation
                            .emit_packet(encode(b"PlayerControlledPump"), 1.0);
                    }
                    self.animation.set_attribute(SettableAttribute {
                        name,
                        value: result.height,
                        normalized: false,
                        sequence_id: -1,
                    });
                }
            }
            (MotionOperation::SetTurning(names), Instance::Turning(instance)) => {
                if phase == 0 {
                    instance.enter();
                } else if phase == 1 {
                    let physical = self
                        .physical
                        .ok_or("SetTurning requires actual PhysOutAnimation and stance outputs")?;
                    let intents = set_turning::Intents {
                        fakie_turn: self.animation.motion_intent("FakieTurn"),
                        mode_0_slide: self.animation.motion_intent("LeftSlide"),
                        mode_1_slide: self.animation.motion_intent("RightSlide"),
                    };
                    //ISkaterAnim v12=82B970D8/v28=82B97140 read live bits29/30.
                    //UpdateRidingFakie may have changed them earlier in this graph tick.
                    let flags = self
                        .animation
                        .skater_animation_flags
                        .ok_or("SetTurning requires actual animation stance flags")?;
                    set_turning::update(
                        instance,
                        &mut self.slide_latch,
                        physical.turning,
                        (flags & 0x2000_0000 != 0, flags & 0x4000_0000 != 0),
                        frame.dt,
                        &self.turning,
                        intents,
                        |attribute, value| {
                            let index = match attribute {
                                set_turning::Attribute::Angle => Some(0),
                                set_turning::Attribute::Direction => Some(1),
                                set_turning::Attribute::Quickness => Some(2),
                                set_turning::Attribute::Speed => Some(3),
                                set_turning::Attribute::Holding => Some(4),
                                _ => None,
                            };
                            if let Some(index) = index {
                                self.animation.set_attribute(SettableAttribute {
                                    name: names[index],
                                    value,
                                    normalized: false,
                                    sequence_id: -1,
                                });
                            } else {
                                self.animation.emit_packet(
                                    encode(if attribute == set_turning::Attribute::Turn {
                                        b"Turn"
                                    } else {
                                        b"Slide"
                                    }),
                                    value,
                                );
                            }
                        },
                    );
                }
            }
            (MotionOperation::Push(_), Instance::Push(instance)) => {
                let physical = self
                    .physical
                    .ok_or("Push behavior requires actual skater physical outputs")?;
                // The attribute sink borrows animation mutably during this
                // callback; retain a snapshot of the same canonical intent map.
                let intents = self.animation.motion_intents.clone();
                let mut context = PushContext {
                    settings: &self.pushing,
                    shared: self
                        .push_state
                        .as_mut()
                        .ok_or("Native push state initialization has not been published")?,
                    motion_intents: &intents,
                    forward_speed: physical.forward_speed,
                    delta_seconds: frame.dt,
                    time_since_teleport: self.riding.time_since_teleport,
                    is_switch: physical.is_switch,
                    foot_frame: physical.foot_frame,
                };
                match phase {
                    0 => instance.begin(&mut context, &mut self.animation),
                    1 => instance.update(&mut context, &mut self.animation)?,
                    _ => instance.end(&mut context),
                }
            }
            (MotionOperation::JumpInto(name), Instance::JumpInto(pending)) => {
                // TU3 82BACDC0: Update consumes the allocation-time latch even
                // if the authored marker is absent. Begin/End are empty.
                if phase == 1 && *pending {
                    self.animation.jump_into(name)?;
                    *pending = false;
                }
            }
            (MotionOperation::Unsupported { kind, name }, _) => {
                return Err(format!("Unsupported MotionGraph {kind:?} {name}"));
            }
            (operation, _) => {
                return Err(format!(
                    "Invalid MotionGraph behavior instance {operation:?}"
                ));
            }
        }
        Ok(())
    }
}
