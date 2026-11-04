//! Prints the nodes of one ambient NPC skater line (for NPC_SKATER_STUCK / _BACKWARDS log lines). Data only.
//! usage: npc_line_dump <assets>/private/living_world/skater_paths/<District>.bin <line id hex> [first node] [last node]
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let pack = args.next().ok_or("Expected the district's skater_paths .bin")?;
    let id = args.next().ok_or("Expected the line id (32 hex digits)")?;
    let first: usize = args.next().map_or(Ok(0), |a| a.parse().map_err(|e| format!("first node: {e}")))?;
    let last: usize = args.next().map_or(Ok(usize::MAX), |a| a.parse().map_err(|e| format!("last node: {e}")))?;
    let bytes = std::fs::read(&pack).map_err(|e| format!("{pack}: {e}"))?;
    let tiles = skate_data::aipath::parse_pack(&bytes).map_err(|e| e.to_string())?;
    let (paths, _) = skate_data::aipath::district_paths(&tiles).map_err(|e| e.to_string())?;
    let hex = |b: &[u8; 16]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    // `near:x,z`: the lines whose first node lies within 3 m of the point (to name a line-end chain's next line).
    if let Some(at) = id.strip_prefix("near:") {
        let v: Vec<f32> = at.split(',').map(|s| s.parse().map_err(|e| format!("near: {e}"))).collect::<Result<_, _>>()?;
        let [x, z] = v[..] else { return Err("near:x,z".into()) };
        for p in &paths {
            let line = skate_data::living_world::replay_line(&p.path);
            if let Some(n) = line.nodes.first() {
                let d = (n.position[0] - x).hypot(n.position[2] - z);
                if d < 3.0 {
                    println!("{} nodes {} first node at [{:.2}, {:.2}, {:.2}] ({d:.2} m)", hex(&p.path.id.0), line.nodes.len(), n.position[0], n.position[1], n.position[2]);
                }
            }
        }
        return Ok(());
    }
    let path = paths.iter().find(|p| hex(&p.path.id.0) == id.to_ascii_lowercase()).ok_or(format!("No line {id}"))?;
    let line = skate_data::living_world::replay_line(&path.path);
    println!("line {} flags {} skill {} nodes {} jumps {}", id, line.flags, line.skill, line.nodes.len(), line.jumps.len());
    println!("node  frames event flags  x        y       z        speed  trick  jump start_velocity");
    for (i, n) in line.nodes.iter().enumerate().skip(first).take(last.saturating_sub(first).saturating_add(1)) {
        let speed = (n.step[0] * n.step[0] + n.step[1] * n.step[1] + n.step[2] * n.step[2]).sqrt() * 60.0;
        let jump = n.jump.and_then(|j| line.jumps.get(j as usize));
        let v = jump.map_or(String::new(), |j| format!("[{:.2}, {:.2}, {:.2}]", j.start_velocity[0], j.start_velocity[1], j.start_velocity[2]));
        println!(
            "{i:4}  {:6} {:5} {:#04x}  {:8.2} {:7.2} {:8.2} {speed:6.2} {:5}  {v}",
            n.frames, n.event, n.flags, n.position[0], n.position[1], n.position[2], line.node_trick(n)
        );
    }
    Ok(())
}
