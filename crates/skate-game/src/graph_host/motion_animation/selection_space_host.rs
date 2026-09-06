//! Deferred native GetAnimTree callback in SelectionSpace::SetAttributes.
use super::*;

impl MotionAnimation {
    pub(super) fn prepare_selection_spaces(
        &mut self,
        tree: &mut PlaybackTree,
        attributes: &[SettableAttribute],
    ) -> Result<(), String> {
        match tree {
            PlaybackTree::SelectionSpace(space) => {
                space.select(attributes)?;
                if !space.child_constructed {
                    let motion = space
                        .current()
                        .ok_or("SelectionSpace has no selected child")?
                        .clone();
                    let (motion, posture) = if self.posture_bank_valid {
                        self.posture
                            .apply::<_, String>((motion, None), |(motion, _), pose| {
                                Ok((motion, Some(pose)))
                            })?
                    } else {
                        (motion, None)
                    };
                    *space.current_mut().unwrap() = self.add_bind_pose(motion, posture)?;
                    space.child_constructed = true;
                }
                self.prepare_selection_spaces(space.current_mut().unwrap(), attributes)?;
            }
            PlaybackTree::BlendSpace(space) => {
                // Type6 SetAttributes82D24078 consumes its own parameters only.
                let _ = space;
            }
            PlaybackTree::PhaseBlend(space) => {
                for child in &mut space.children {
                    self.prepare_selection_spaces(child, attributes)?;
                }
            }
            PlaybackTree::Transition(transition) => {
                // PlaybackTransition::set_attributes follows this same to/from order.
                self.prepare_selection_spaces(&mut transition.to, attributes)?;
                if transition.settings.kind == 4 && !transition.complete() {
                    self.prepare_selection_spaces(&mut transition.from, attributes)?;
                }
            }
            PlaybackTree::BindPose { motion, .. } => {
                self.prepare_selection_spaces(motion, attributes)?
            }
            PlaybackTree::Clip { .. } => {}
        }
        Ok(())
    }
}
