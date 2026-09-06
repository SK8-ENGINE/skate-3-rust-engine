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
    if group_pair_allowed(board_group, collision.assembly_group) {
        for a in board {
            for b in rider {
                let CollisionBody::Attached(part) = b.body else {
                    unreachable!()
                };
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
    match (a, b) {
        (0, 0 | 2..=7) | (2..=7, 0) => true,
        (4, 5) | (5, 4) => false,
        (4..=7, 4..=7) => true,
        _ => false,
    }
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
