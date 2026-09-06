//! Original single-player SkaterSkaterCollisionPipeline8276556C.
//! Board construction precedes Skeleton construction in82DB18A0. Preserve
//! that assembly order: board self (all culled), board/rider, rider self.
use skate_core::physics::{
    board_step::{BoardCollision, CollisionBody},
    board_world::BoardWorldVolume,
    contact::{RetailContactInput, combine_contact_materials},
    skeleton_body::SkeletonCollisionMode,
    world_contact::{PrimitivePairSettings, primitive_pair_contacts},
};

pub(super) fn append(
    contacts: &mut Vec<BoardCollision>,
    board: &[BoardWorldVolume],
    rider: &[BoardWorldVolume],
    board_group: u32,
    collision: &SkeletonCollisionMode,
) -> Result<(), String> {
    //82765EF0 initializes the single-player (Island flags3) group bitmap.
    //Rows1/2/3 are culled except2/3 against0;4<->5 is always culled.
    //Its later extra4..7 exclusions run only for the online/non3 branch.
    if board_group > 7 || collision.assembly_group > 7 {
        return Err("Skater assembly collision requires its original group-table extension".into());
    }
    {
        for a in board {
            for b in rider {
                let CollisionBody::Attached(part) = b.body else {
                    unreachable!()
                };
                // The per-part group is independently assigned in Biped mode:
                // hands5, legs17, torso18 and the two controller capsules20.
                // Using assembly6 here makes the carried board collide with
                // its own hand drive and the capsules that feed root correction.
                if !group_pair_allowed(board_group, collision.parts[part].part_group) {
                    continue;
                }
                //Serialized volume+212 is source Volume+84 group.
                //Stock board child constructors leave that field zero.
                if !group_pair_allowed(0, collision.parts[part].volume_group) {
                    continue;
                }
                append_pair(contacts, a, b);
            }
        }
    }
    //8277AE80 suppresses the reversed assembly pair. The same-assembly
    //branch8277B2C0 instead visits both directed primitive pairs and uses
    //the skeleton's part bitmap. Board82C0B560 culls all7x7 self pairs.
    for a in rider {
        let CollisionBody::Attached(part_a) = a.body else {
            unreachable!()
        };
        for b in rider {
            let CollisionBody::Attached(part_b) = b.body else {
                unreachable!()
            };
            if part_a != part_b && !collision.self_culling[part_a][part_b] {
                append_pair(contacts, a, b);
            }
        }
    }
    Ok(())
}

fn group_pair_allowed(a: u32, b: u32) -> bool {
    // Single-player21x21 bitmap constructed by82765EF0 through the
    // Island.flags==3 branch. One allowed bit per column, inverted from the
    // native culling bits. Keep special Biped groups instead of truncating to8.
    const ALLOWED: [u32; 21] = [
        0x1876fd, 0, 0x120001, 1, 0x79d1, 0x1079e1, 0x1079f1,
        0x279f1, 0x1676f0, 0x7101, 0x5101, 0x1008f0, 0x1d77f1,
        0x233f1, 0xa57f1, 0, 0x1000, 0x6184, 0x1100, 0x5001, 0x1965,
    ];
    b < 21 && ALLOWED.get(a as usize).is_some_and(|row| row & (1 << b) != 0)
}

fn append_pair(contacts: &mut Vec<BoardCollision>, a: &BoardWorldVolume, b: &BoardWorldVolume) {
    //8277B0F0 and8277B2C0 enter the same8277A508 primitive walker with
    //identical padding/triangle settings. Keep A/B orientation and point order.
    let Some(manifold) = primitive_pair_contacts(
        a.primitive,
        b.primitive,
        PrimitivePairSettings::skater_self_collision(),
    ) else {
        return;
    };
    let material = combine_contact_materials(a.material, b.material);
    for points in &manifold.points[..manifold.count] {
        contacts.push(BoardCollision {
            body_a: a.body,
            body_b: b.body,
            contact: RetailContactInput {
                position_on_a: points.a,
                position_on_b: points.b,
                normal: manifold.normal,
                restitution: material.restitution,
                static_friction: material.static_friction,
                dynamic_friction: material.dynamic_friction,
                //8277A7B4/B8 packs original Volume88 tags, both zero for
                //stock skater/board constructors (not the part indices).
                tag: 0,
            },
        });
    }
}
