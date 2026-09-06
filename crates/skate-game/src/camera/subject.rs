//! Typed physics/animation publication boundary for normal subject82DF69C0.
//! The simulation supplies current physical outputs; camera history stays here.
use bevy::prelude::Resource;
use skate_core::camera::{
    AnchorInputs, Anchors, CameraMan, Compass, CompassPoseInputs, CompassSettings,
    ManagerSubject, ReferencePointInputs, SubjectPoseInputs, SubjectPosePublisher,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraSubjectSnapshot {
    /// Physical flags, trajectory, steering, root and other direct outputs.
    /// The camera overwrites this value's transform, reference_positions,
    /// anchors and compass with the source publishers below before use.
    pub subject: ManagerSubject,
    pub pose: SubjectPoseInputs,
    pub anchors: AnchorInputs,
    /// Bone positions and raw COM; tracked_anchor, incline_normal and damped
    /// COM are filled by their camera owners immediately before publication.
    pub reference_points: ReferencePointInputs,
    pub compass: CompassPoseInputs,
    pub graph: super::graph_subject::CameraGraphSubject,
}

/// Root's simulation publisher replaces this after a complete physical/pose
/// update. None means there is no valid gameplay subject yet.
#[derive(Resource, Default)]
pub(crate) struct CameraSubjectFrame {
    pub snapshot: Option<CameraSubjectSnapshot>,
    pub generation: u64,
}

pub(super) struct SubjectPublisher {
    pose: SubjectPosePublisher,
    anchors: Anchors,
    compass: Compass,
}
impl SubjectPublisher {
    pub fn new() -> Self {
        Self { pose: SubjectPosePublisher::new(), anchors: Anchors::new(), compass: Compass::new() }
    }

    pub fn publish(&mut self, mut input: CameraSubjectSnapshot, manager: &CameraMan,
        settings: CompassSettings) -> ManagerSubject {
        let pose = self.pose.publish(input.pose);
        input.subject.rig.transform = pose.transform;
        input.subject.rig.skeleton_root = input.pose.skeleton_root;
        input.anchors.damped_center_of_mass = pose.damped_center_of_mass;
        self.anchors.update(input.anchors);
        input.subject.anchors = self.anchors.entries;
        input.reference_points.damped_centre_of_mass = pose.damped_center_of_mass;
        input.reference_points.tracked_anchor = manager.rig.anchor.position;
        input.reference_points.incline_normal = manager.state.incline_normal;
        input.subject.rig.reference_positions = input.reference_points.positions();
        let selected = if manager.shots.current().name.is_empty() { 5 }
            else { manager.shots.current().shot.compass_north };
        let compass_input = input.compass.bind(&input.subject, manager.frame.position, selected);
        input.subject.compass = self.compass.update(
            if input.subject.reset != 0 { 0.0 } else { f32::from_bits(0x3c88_8889) },
            compass_input, settings);
        input.subject
    }
}
