//! Retained, mod-owned 2D canvases. No extra cameras, fonts, or render assets.
use bevy::prelude::*;
use skate_mods::presentation::{CanvasAnchor, CanvasKind, CanvasOptions};
use std::collections::BTreeMap;

pub(super) struct Canvas {
    root: Entity,
    nodes: BTreeMap<String, (Entity, CanvasKind)>,
    options: CanvasOptions,
}
pub(super) fn remove(world: &mut World, canvases: &mut BTreeMap<(String,String),Canvas>, key: &(String,String)) {
    if let Some(c)=canvases.remove(key) { world.despawn(c.root); }
}
pub(super) fn clear_owner(world: &mut World, canvases: &mut BTreeMap<(String,String),Canvas>, owner: Option<&str>) {
    let keys: Vec<_>=canvases.keys().filter(|(o,_)| owner.is_none_or(|id| id==o)).cloned().collect();
    for key in keys { remove(world,canvases,&key); }
}
pub(super) fn set(world: &mut World, canvases: &mut BTreeMap<(String,String),Canvas>, owner:&str, key:String, options:CanvasOptions) -> Result<(),String> {
    let slot=(owner.to_owned(),key);
    if !canvases.contains_key(&slot) && canvases.keys().filter(|(o,_)| o==owner).count()>=8 {
        return Err("8 canvases per mod maximum".into());
    }
    let other_items:usize=canvases.iter().filter(|(k,_)| k.0==owner && *k!=&slot)
        .map(|(_,c)|c.options.items.len()).sum();
    if other_items+options.items.len()>128 { return Err("128 canvas items per mod maximum".into()); }
    if canvases.get(&slot).is_some_and(|c| world.get::<Node>(c.root).is_none()) {
        canvases.remove(&slot);
    }
    let c=canvases.entry(slot).or_insert_with(|| {
        let root=world.spawn((Name::new("Lua screen canvas"),GlobalZIndex(6),bevy::ui::FocusPolicy::Pass,
            Node{position_type:PositionType::Absolute,overflow:Overflow::clip(),..default()})).id();
        Canvas{root,nodes:BTreeMap::new(),options:CanvasOptions::default()}
    });
    let stale:Vec<_>=c.nodes.keys().filter(|k| !options.items.iter().any(|i| &i.key==*k)).cloned().collect();
    for key in stale { if let Some((e,_))=c.nodes.remove(&key){ world.despawn(e); } }
    for item in &options.items {
        if c.nodes.get(&item.key).is_some_and(|(e,k)| *k!=item.kind || world.get::<Node>(*e).is_none()) {
            if let Some((e,_))=c.nodes.remove(&item.key){ world.despawn(e); }
        }
        let root=c.root;
        let (e,_)=*c.nodes.entry(item.key.clone()).or_insert_with(|| {
            let e=world.spawn((Node::default(),ChildOf(root),bevy::ui::FocusPolicy::Pass)).id(); (e,item.kind)
        });
        let color=Color::srgba(item.color[0],item.color[1],item.color[2],item.color[3]);
        match item.kind {
            CanvasKind::Rect => { world.entity_mut(e).insert(BackgroundColor(color)); }
            CanvasKind::Text => {
                // Reuse entities; replace only changed text, avoiding needless reshaping.
                if world.get::<Text>(e).is_none_or(|t| t.0!=item.text) {
                    world.entity_mut(e).insert(Text::new(item.text.clone()));
                }
                world.entity_mut(e).insert(TextColor(color));
            }
        }
    }
    c.options=options;
    Ok(())
}
/// Run after Lua on_update/apply, including while paused, so HUD cannot cover menus.
pub(super) fn present(world:&mut World, canvases:&BTreeMap<(String,String),Canvas>, hidden:bool) {
    let viewport=world.query::<&Window>().iter(world).next()
        .map(|w|Vec2::new(w.width(),w.height())).unwrap_or(Vec2::new(1280.0,720.0));
    let ui_scale=world.get_resource::<UiScale>().map(|s|s.0).unwrap_or(1.0).max(0.01);
    let view=viewport/ui_scale;
    for c in canvases.values() {
        let o=&c.options;
        let auto=(view.y/900.0).clamp(0.65,1.5);
        let requested=auto*o.scale;
        let fit_x=view.x/(o.size[0]+2.0*o.offset[0]);
        let fit_y=view.y/(o.size[1]+2.0*o.offset[1]);
        let scale=requested.min(fit_x).min(fit_y).max(0.05);
        let mut node=Node{position_type:PositionType::Absolute,
            width:px(o.size[0]*scale),height:px(o.size[1]*scale),overflow:Overflow::clip(),
            display:if hidden || !o.visible {Display::None}else{Display::Flex},..default()};
        match o.anchor {
            CanvasAnchor::TopLeft => {node.left=px(o.offset[0]*scale);node.top=px(o.offset[1]*scale);}
            CanvasAnchor::TopRight => {node.right=px(o.offset[0]*scale);node.top=px(o.offset[1]*scale);}
            CanvasAnchor::BottomLeft => {node.left=px(o.offset[0]*scale);node.bottom=px(o.offset[1]*scale);}
            CanvasAnchor::BottomRight => {node.right=px(o.offset[0]*scale);node.bottom=px(o.offset[1]*scale);}
        }
        if world.get::<Node>(c.root).is_some(){world.entity_mut(c.root).insert(node);}
        for (index,item) in o.items.iter().enumerate() {
            let Some((e,_))=c.nodes.get(&item.key) else {continue};
            if world.get::<Node>(*e).is_none(){continue;}
            world.entity_mut(*e).insert(ZIndex(index as i32));
            world.entity_mut(*e).insert(Node{position_type:PositionType::Absolute,
                left:px(item.position[0]*scale),top:px(item.position[1]*scale),
                width:px(item.size[0]*scale),height:px(item.size[1]*scale),..default()});
            if item.kind==CanvasKind::Text && world.get::<TextFont>(*e)
                .is_none_or(|font| (font.font_size-item.font_size*scale).abs()>0.01) {
                world.entity_mut(*e).insert(TextFont{font_size:item.font_size*scale,..default()});
            }
        }
    }
}
