//! Stock tree ownership and persistent playback used directly by MotionHost.
mod selection_space_host;
mod tree_builder;
use skate_core::animation::posture::{PendingPosture, PosturePose};
use skate_core::animation::{
    clip_clock::AdvanceResult,
    output::attributes::{AnimationAttribute, AttributeName, MotionGraphAttribute},
    phase_blend::PhaseBlend,
    playback::{PlaybackRequest, PlaybackService},
    playback_clip::{ClipAttribute, PlaybackClip},
    playback_parameters::{AttributeSink, ParameterInputs, SettableAttribute, SettableAttributes},
    playback_transition::PlaybackTransition,
    playback_tree::{Evaluation, PlaybackTree, PoseCommand},
    skeleton_input::name::encode,
};
use skate_core::graph::intents::IntentMap;
use skate_data::animation_metadata::{AnimationMetadata, TreeMetadata};
use tree_builder::build;

pub struct MotionAnimation {
    metadata: AnimationMetadata,
    current: Option<PlaybackTree>,
    pub channels: super::motion_channels::MotionChannels,
    pub current_name: Option<String>,
    pub motion_intents: IntentMap,
    pub filtered_intents: IntentMap,
    pub motion_attributes: Vec<MotionGraphAttribute>,
    pub construction_values: Vec<(AttributeName, AttributeName)>,
    pub posture: PendingPosture,
    pub posture_bank_valid: bool,
    ///Actual fullSkaterAnim15180 flags. AddBindPose consumes bits22/21 on
    ///construction; the owner publishes the updated flags back to the actor.
    pub skater_animation_flags: Option<u32>,
    attribute_mirror: std::sync::Arc<skate_core::animation::playback_attributes::AttributeMirror>,
    settable: SettableAttributes,
    tree_attributes: Vec<AnimationAttribute>,
    property: AdvanceResult,
}
impl MotionAnimation {
    pub fn reset_from_stock(&mut self) {
        self.current = None;
        self.current_name = None;
        self.channels.reset_from_stock();
        self.motion_intents.clear();
        self.filtered_intents.clear();
        self.motion_attributes.clear();
        self.tree_attributes.clear();
        self.settable.clear();
        self.skater_animation_flags = Some(0);
    }
    pub fn from_metadata(metadata: AnimationMetadata) -> Self {
        Self {
            metadata,
            current: None,
            channels: super::motion_channels::MotionChannels::default(),
            current_name: None,
            motion_intents: IntentMap::new(),
            filtered_intents: IntentMap::new(),
            motion_attributes: Vec::new(),
            construction_values: Vec::new(),
            posture: PendingPosture::default(),
            posture_bank_valid: false,
            skater_animation_flags: None,
            attribute_mirror: std::sync::Arc::new(
                skate_core::animation::playback_attributes::AttributeMirror(Vec::new()),
            ),
            settable: SettableAttributes::default(),
            tree_attributes: Vec::new(),
            property: AdvanceResult {
                crossed_end: false,
                overshoot: -1.0,
                remaining_before_wrap: -1.0,
            },
        }
    }
    ///Share the already loaded stock hierarchy with playback; no frame reload.
    pub fn set_hierarchy(
        &mut self,
        names: &[String],
        mirror_indices: &[i32],
    ) -> Result<(), String> {
        if names.len() != mirror_indices.len() {
            return Err("Animation hierarchy mirror count differs from bone count".into());
        }
        let mut pairs = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let target = match mirror_indices[index] {
                -1 => name,
                i if i >= 0 => names.get(i as usize).ok_or("Invalid mirrored bone index")?,
                _ => return Err("Invalid negative mirrored bone index".into()),
            };
            pairs.push((encode(name.as_bytes()), encode(target.as_bytes())));
        }
        self.attribute_mirror = std::sync::Arc::new(
            skate_core::animation::playback_attributes::AttributeMirror(pairs),
        );
        Ok(())
    }
    pub fn tree_attributes(&self) -> &[AnimationAttribute] {
        &self.tree_attributes
    }
    pub fn property(&self) -> AdvanceResult {
        self.property
    }
    pub fn current_time(&self) -> Result<f32, String> {
        self.current
            .as_ref()
            .map(PlaybackTree::time)
            .ok_or_else(|| "No current animation tree".into())
    }
    pub fn current_length(&self) -> Result<f32, String> {
        self.current
            .as_ref()
            .map(PlaybackTree::length)
            .ok_or_else(|| "No current animation tree".into())
    }
    pub fn in_transition(&self) -> bool {
        self.current.as_ref().is_some_and(has_transition)
    }
    ///825310F0: retire completed transitions before advancing; reset property
    /// once at the root. Graph evaluation, parametrization and pose evaluation
    /// remain separate calls so the actor schedule owns their meaningful order.
    pub fn advance(&mut self, dt: f32, phase: f32) {
        self.channels.retire();
        self.current = self.current.take().map(prune);
        self.property = AdvanceResult {
            crossed_end: false,
            overshoot: -1.0,
            remaining_before_wrap: -1.0,
        };
        if let Some(tree) = &mut self.current {
            tree.advance(dt, phase, &mut self.property);
        }
        self.channels.advance(dt, phase);
    }
    pub fn apply_parameters(&mut self) -> Result<(), String> {
        let attributes = self.settable.entries().to_vec();
        if let Some(mut tree) = self.current.take() {
            let result = self.prepare_selection_spaces(&mut tree, &attributes);
            self.current = Some(tree);
            result?;
        }
        let mut channels = std::mem::take(&mut self.channels);
        let result =
            channels.prepare_trees(|tree| self.prepare_selection_spaces(tree, &attributes));
        self.channels = channels;
        result?;
        if let Some(tree) = &mut self.current {
            tree.set_attributes(self.settable.entries())?;
        }
        self.channels.set_attributes(self.settable.entries())?;
        self.settable.clear();
        Ok(())
    }
    pub fn evaluate_pose(&mut self, parameters: Evaluation) -> Result<Vec<PoseCommand>, String> {
        let mut output = Vec::new();
        let tree = self
            .current
            .as_mut()
            .ok_or("MotionGraph has not selected an animation")?;
        tree.evaluate(parameters, true, &mut output)?;
        self.channels.evaluate(parameters, &mut output)?;
        Ok(output)
    }
    pub fn refresh_tree_attributes(&mut self) -> Result<(), String> {
        self.tree_attributes = self.channels.attributes(
            match &self.current {
                Some(tree) => tree.attributes(15)?,
                None => Vec::new(),
            },
            15,
        )?;
        Ok(())
    }
    pub fn begin_graph_update(&mut self) {
        self.motion_attributes.clear();
    }
    pub fn attach(&mut self, intent: &str, attribute: AttributeName, set: bool) {
        if let Some(value) = self.motion_intent(intent) {
            self.motion_attributes.push(MotionGraphAttribute {
                name: attribute,
                value,
            });
            if set {
                self.set_attribute(SettableAttribute {
                    name: attribute,
                    value,
                    normalized: false,
                    sequence_id: -1,
                });
            }
        }
    }
    pub fn emit_packet(&mut self, name: AttributeName, value: f32) {
        self.motion_attributes
            .push(MotionGraphAttribute { name, value });
    }
    pub fn build_tree(&self, name: &str) -> Result<PlaybackTree, String> {
        build(
            &self.metadata,
            name,
            &self.construction_values,
            &mut Vec::new(),
        )
    }
    pub fn new_channel(
        &mut self,
        name: &str,
        animation: &str,
        settings: skate_core::animation::channel_playback::ChannelSettings,
    ) -> Result<bool, String> {
        if self.channels.has(name) {
            return Ok(false);
        }
        let mut motion = self.build_tree(animation)?;
        motion.set_speed(settings.speed);
        let mut tree = self.add_bind_pose(motion, None)?;
        if settings.mirrored {
            if let PlaybackTree::BindPose { mirror_modes, .. } = &mut tree {
                mirror_modes.push(2);
            }
        }
        self.channels.insert(name.into(), tree, settings);
        Ok(true)
    }
    pub fn transition_channel(
        &mut self,
        name: &str,
        animation: &str,
        settings: skate_core::animation::channel_playback::ChannelSettings,
        transition: skate_core::animation::playback::TransitionSettings,
        resurrect: bool,
        create_missing: bool,
    ) -> Result<bool, String> {
        if !self.channels.has(name) {
            return if create_missing {
                self.new_channel(name, animation, settings)
            } else {
                Ok(false)
            };
        }
        if !self.channels.can_transition(name, resurrect) {
            return Ok(false);
        }
        let mut motion = self.build_tree(animation)?;
        motion.set_speed(settings.speed);
        let mut tree = self.add_bind_pose(motion, None)?;
        if settings.mirrored {
            if let PlaybackTree::BindPose { mirror_modes, .. } = &mut tree {
                mirror_modes.push(2);
            }
        }
        self.channels
            .transition(name, tree, settings, transition, resurrect);
        Ok(true)
    }
    fn add_bind_pose(
        &mut self,
        motion: PlaybackTree,
        posture: Option<PosturePose>,
    ) -> Result<PlaybackTree, String> {
        let flags = self
            .skater_animation_flags
            .ok_or("Animation construction requires actual SkaterAnim flags")?;
        let mut mirror_modes = Vec::new();
        if flags & 0x40000000 != 0 {
            mirror_modes.push(2);
        }
        if flags & 0x00400000 != 0 {
            mirror_modes.push(u32::from(flags & 0x00200000 == 0) + 1);
        }
        self.skater_animation_flags = Some(flags & !0x00600000);
        if !mirror_modes.is_empty() && self.attribute_mirror.0.is_empty() {
            return Err("Mirrored animation requires the stock bone hierarchy".into());
        }
        Ok(PlaybackTree::BindPose {
            motion: Box::new(motion),
            posture,
            board_backwards: flags & 0x80000000 != 0,
            mirror_modes,
            attribute_mirror: self.attribute_mirror.clone(),
        })
    }
}
fn has_transition(tree: &PlaybackTree) -> bool {
    match tree {
        PlaybackTree::Transition(_) => true,
        PlaybackTree::PhaseBlend(tree) => tree.children.iter().any(has_transition),
        PlaybackTree::BlendSpace(tree) => tree.children.iter().any(has_transition),
        PlaybackTree::SelectionSpace(tree) => tree.current().is_some_and(has_transition),
        PlaybackTree::BindPose { motion, .. } => has_transition(motion),
        _ => false,
    }
}
fn prune(tree: PlaybackTree) -> PlaybackTree {
    match tree {
        PlaybackTree::Transition(mut transition) => {
            if transition.complete() {
                prune(*transition.to)
            } else {
                transition.from = Box::new(prune(*transition.from));
                transition.to = Box::new(prune(*transition.to));
                PlaybackTree::Transition(transition)
            }
        }
        PlaybackTree::PhaseBlend(mut tree) => {
            tree.children = tree.children.into_iter().map(prune).collect();
            PlaybackTree::PhaseBlend(tree)
        }
        PlaybackTree::BlendSpace(mut tree) => {
            tree.children = tree.children.into_iter().map(prune).collect();
            PlaybackTree::BlendSpace(tree)
        }
        PlaybackTree::SelectionSpace(mut tree) => {
            tree.candidates = tree
                .candidates
                .into_iter()
                .map(|mut candidate| {
                    candidate.tree = prune(candidate.tree);
                    candidate
                })
                .collect();
            PlaybackTree::SelectionSpace(tree)
        }
        PlaybackTree::BindPose {
            motion,
            posture,
            board_backwards,
            mirror_modes,
            attribute_mirror,
        } => PlaybackTree::BindPose {
            motion: Box::new(prune(*motion)),
            posture,
            board_backwards,
            mirror_modes,
            attribute_mirror,
        },
        other => other,
    }
}
impl ParameterInputs for MotionAnimation {
    fn motion_intent(&self, name: &str) -> Option<f32> {
        self.motion_intents.get(name).copied()
    }
    fn filtered_intent(&self, name: &str) -> Option<f32> {
        self.filtered_intents.get(name).copied()
    }
    fn last_attribute(
        &mut self,
        name: AttributeName,
    ) -> Result<Option<AnimationAttribute>, String> {
        Ok(self
            .tree_attributes
            .iter()
            .find(|a| a.name == name)
            .copied())
    }
}
impl AttributeSink for MotionAnimation {
    fn set_attribute(&mut self, attribute: SettableAttribute) {
        self.settable.set_attribute(attribute);
    }
}
impl super::pushing::PushAnimationSink for MotionAnimation {
    fn set_attribute(&mut self, a: super::pushing::SettableAttribute) {
        AttributeSink::set_attribute(
            self,
            SettableAttribute {
                name: encode(a.name.as_bytes()),
                value: a.value,
                normalized: a.normalized,
                sequence_id: a.sequence_id,
            },
        );
    }
}
impl PlaybackService for MotionAnimation {
    fn set_construction_value(&mut self, name: AttributeName, value: AttributeName) {
        if let Some(entry) = self
            .construction_values
            .iter_mut()
            .find(|entry| entry.0 == name)
        {
            entry.1 = value;
        } else {
            self.construction_values.push((name, value));
        }
    }
    fn set_posture_enabled(&mut self, enabled: bool) {
        self.posture.set_requested(enabled);
    }
    fn play(&mut self, request: PlaybackRequest) -> Result<bool, String> {
        if !(1..=4).contains(&request.transition.kind) {
            return Ok(false);
        }
        if request.transition.kind == 1 {
            self.current = None;
        }
        let motion = self.build_tree(&request.animation)?;
        //SkaterAnim::GetAnimTree82B980A0 calls AddBindPose82B98118 before
        //Play/Blend set time/speed and before transition initialization.
        let (motion, posture) = if self.posture_bank_valid {
            self.posture
                .apply::<_, String>((motion, None), |(motion, _), pose| Ok((motion, Some(pose))))?
        } else {
            (motion, None)
        };
        let mut tree = self.add_bind_pose(motion, posture)?;
        tree.set_speed(request.speed);
        let prior = self.current.take();
        //82D186F8's no-current branch calls Play with start0.
        if request.start_time > 0.0 && (request.transition.kind == 1 || prior.is_some()) {
            tree.set_time(request.start_time);
        }
        self.current = Some(match prior {
            Some(mut from) => {
                if request.transition.under != 0 {
                    if let PlaybackTree::Transition(transition) = &mut from {
                        let to = std::mem::replace(&mut transition.to, Box::new(tree.clone()));
                        transition.to = Box::new(PlaybackTree::Transition(
                            PlaybackTransition::new(*to, tree, request.transition),
                        ));
                        from
                    } else {
                        PlaybackTree::Transition(PlaybackTransition::new(
                            from,
                            tree,
                            request.transition,
                        ))
                    }
                } else {
                    PlaybackTree::Transition(PlaybackTransition::new(
                        from,
                        tree,
                        request.transition,
                    ))
                }
            }
            None => tree,
        });
        self.current_name = Some(request.animation);
        Ok(true)
    }
}
