//! Original action-stream validation with supplied data, without game systems.
#[path = "../../skate-game/src/apt_display.rs"]
mod apt_display;
#[path = "../../skate-game/src/apt_movie.rs"]
mod apt_movie;
#[path = "../../skate-game/src/apt_scene.rs"]
mod apt_scene;
#[path = "../../skate-game/src/apt_text.rs"]
mod apt_text;
#[path = "../../skate-game/src/apt_vm.rs"]
mod apt_vm;
#[path = "../../skate-game/src/hud_runtime.rs"]
mod hud_runtime;
use apt_vm::Value;
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Expected owned trickdisplay.json")?;
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut input = hud_runtime::Input {
        sequence_score: 0,
        line_score: 0,
        sequence_timer: 0,
        line_time: 0.0,
        line_capacity: 400.0,
        multiplier: 1.0,
        clean: false,
        sketchy: false,
        stance: [false; 4],
        trick_name: String::new(),
        trick_metrics: std::array::from_fn(|_| Value::Undefined),
        context_tricks: Vec::new(),
    };
    let mut runtime = hud_runtime::Runtime::load(&json, input.clone())?;
    let shapes: apt_scene::Shapes =
        serde_json::from_value(json["shapes"].clone()).map_err(|e| e.to_string())?;
    let mut max_draws = 0;
    let mut saw_native_shadow_pair = false;
    for tick in 0..1800 {
        let new_trick = tick % 180 == 10;
        if new_trick {
            input.trick_name = "Kickflip".into();
            input.trick_metrics = [
                Value::Text(input.trick_name.clone()),
                Value::Number(0.0),
                Value::Number(0.0),
                Value::Bool(false),
                Value::Bool(false),
            ];
            input.sequence_score += 100;
            input.line_score += 100;
            input.clean = tick % 360 == 10;
            input.sketchy = !input.clean;
            input.line_time = 8.0;
            input.multiplier = if input.multiplier == 1.5 { 2.0 } else { 1.5 };
        }
        input.line_time = (input.line_time - 1.0 / 60.0).max(0.0);
        input.sequence_timer = input.line_time as i32;
        runtime
            .update(input.clone(), new_trick, false, tick % 180 == 120)
            .map_err(|e| format!("Data frame {tick}: {e}"))?;
        let draws = apt_scene::draw(&runtime.bindings.movie, &runtime.vm, &shapes)?;
        max_draws = max_draws.max(draws.len());
        saw_native_shadow_pair |= draws.windows(2).any(|pair| {
            pair[0].texture.contains("futurashadow")
                && pair[0].multiply[..3] == [0., 0., 0.]
                && pair[1].texture.contains("futuraheavy")
                && pair[1].multiply[0] > 0.
        });

        if draws
            .iter()
            .flat_map(|d| &d.vertices)
            .any(|v| v.position.iter().chain(v.uv.iter()).any(|x| !x.is_finite()))
        {
            return Err("Nonfinite authored draw geometry".into());
        }
    }
    if !saw_native_shadow_pair {
        return Err("Missing native black shadow / heavy foreground pair".into());
    }
    println!(
        "Original HUD action audit passed 1800 authored frames; {} object slots, {} display instances",
        runtime.vm.objects.len(),
        runtime.bindings.movie.instances.len()
    );
    println!("Validated glyph/shape traversal: maximum {max_draws} draw batches");
    Ok(())
}
