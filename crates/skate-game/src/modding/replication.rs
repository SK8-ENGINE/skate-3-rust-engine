//! Generic, package-verified body/scene/node replication. No remote Lua runs,
//! no platform-specific path exists, and keys are scoped by authenticated peer.
//! The existing APPLICATION transport provides sequencing, ACK/retry and late join.
mod wire;
use super::{Mods,graphics};
use bevy::prelude::*;
use serde::{Deserialize,Serialize};
use skate_mods::scene::{GraphicsDefinition,NodeState,TransformState,valid_quaternion,valid_vector};
use std::{collections::{BTreeMap,BTreeSet},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use wire::*;

type Slot=(u64,String,String);
const RECORD_BUDGET:usize=224; // leave room for ordinary sdk.net state
const MAX_REMOTE_BODIES:usize=32;
const MAX_REMOTE_GRAPHICS:usize=64;
const MAX_REMOTE_NODES:usize=128;
const BLEND:f32=0.06;
const EXTRAPOLATION:f32=0.10;

pub(super) struct State {
    epoch:u64, connection:u64, started:Instant, last_send:Instant,
    definitions:BTreeMap<u64,CachedDefinition>,
    bodies:BTreeMap<Slot,Replica>,
    graphics:BTreeMap<Slot,(u64,u64)>,
    node_sequences:BTreeMap<(String,String,String),(u64,u64,u32)>,
    attachments:BTreeMap<u64,Attachment>,
    warnings:BTreeSet<String>, last_problem:String,
    pub status:String,
}
impl Default for State {
    fn default() -> Self {
        let now=Instant::now();
        Self { epoch:(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64).max(1),
            connection:0,started:now,last_send:now-Duration::from_secs(1),definitions:BTreeMap::new(),
            bodies:BTreeMap::new(),graphics:BTreeMap::new(),node_sequences:BTreeMap::new(),attachments:BTreeMap::new(),
            warnings:BTreeSet::new(),last_problem:String::new(),status:String::new() }
    }
}
struct CachedDefinition { revision:usize,hash:u64,chunks:Vec<Vec<u8>> }
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyPose { p:[f32;3],q:[f32;4],v:[f32;3],w:[f32;3],com:[f32;3],stamp:u64 }
impl BodyPose {
    fn valid(&self) -> bool {
        valid_vector(&self.p,100_000.) && valid_quaternion(&self.q)
            && valid_vector(&self.v,1000.) && valid_vector(&self.w,10_000.) && valid_vector(&self.com,2000.)
    }
}
struct Replica {
    id:u64,epoch:u64,instance:u64,revision:u64,seq:u32,
    pose:BodyPose,received:Instant,error_position:Vec3,error_axis:Vec3,
}
impl Replica {
    fn sample(&self) -> (Transform,Vec3,Vec3) {
        let age=self.received.elapsed().as_secs_f32().min(EXTRAPOLATION);
        let base_q=Quat::from_array(self.pose.q).normalize();
        let base_v=Vec3::from_array(self.pose.v);let base_w=Vec3::from_array(self.pose.w);
        let com=Vec3::from_array(self.pose.com);
        let rotation=(Quat::from_scaled_axis(base_w*age)*base_q).normalize();
        let position=Vec3::from_array(self.pose.p)+base_q*com+base_v*age-rotation*com;
        let weight=(1.-age/BLEND).clamp(0.,1.);
        let correction=Quat::from_scaled_axis(self.error_axis*weight);
        let q=(correction*rotation).normalize();
        let t=Transform::from_translation(position+self.error_position*weight).with_rotation(q);
        if age>=EXTRAPOLATION { return (t,Vec3::ZERO,Vec3::ZERO); }
        let w=if weight>0. { correction*base_w-self.error_axis/BLEND } else { base_w };
        // COM velocity includes the derivative of the bounded origin/orientation
        // correction, so contact velocity agrees with the displayed motion.
        let v=if weight>0. {
            base_v-base_w.cross(rotation*com)+w.cross(q*com)-self.error_position/BLEND
        } else { base_v };
        (t,v,w)
    }
    fn receive(&mut self,pose:BodyPose,seq:u32) {
        if seq==self.seq {return;}
        let old=self.sample().0;
        let new_p=Vec3::from_array(pose.p);let new_q=Quat::from_array(pose.q).normalize();
        let error=old.translation-new_p;
        // Reset/large discontinuities are not interpolated through the level.
        if error.length_squared()>25. { self.error_position=Vec3::ZERO;self.error_axis=Vec3::ZERO; }
        else {
            self.error_position=error;
            let mut dq=(old.rotation*new_q.conjugate()).normalize();if dq.w<0. {dq=-dq;}
            self.error_axis=dq.to_scaled_axis();
        }
        self.pose=pose;self.seq=seq;self.received=Instant::now();
    }
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphicRecord {
    definition:GraphicsDefinition,transform:TransformState,visible:bool,body_instance:Option<u64>,
}
#[derive(Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentRecord { offset:[f32;3],hidden:bool }
struct Attachment { owner:String,key:String,epoch:u64,instance:u64,record:AttachmentRecord }
fn owner(peer:u64,package:&str) -> String { format!("@{peer}:{package}") }
fn issue(state:&mut State,message:String) {
    state.last_problem=message.clone();
    if state.warnings.len()<128 && state.warnings.insert(message.clone()) {warn!("Mod replication: {message}");}
}
fn make(kind:u8,package:&str,key:&str,fp:u64,epoch:u64,instance:u64,payload:Vec<u8>) -> Envelope {
    Envelope {kind,owner:package.into(),key:key.into(),node:String::new(),fingerprint:fp,epoch,instance,
        revision:0,index:0,count:0,payload}
}
fn push_group(out:&mut BTreeMap<String,Vec<u8>>,packets:Vec<Envelope>,state:&mut State,label:&str) {
    let mut encoded=Vec::new();
    for packet in packets {
        let Some(bytes)=packet.encode() else {issue(state,format!("{label}: record exceeds protocol bounds"));return;};
        encoded.push((packet.key(),bytes));
    }
    if out.len()+encoded.len()>RECORD_BUDGET {
        issue(state,format!("{label}: {RECORD_BUDGET}-record mod replication budget exceeded"));return;
    }
    out.extend(encoded);
}
fn outgoing(mods:&Mods,state:&mut State) -> BTreeMap<String,Vec<u8>> {
    let mut out=BTreeMap::new();
    let session=Envelope::session(state.epoch);out.insert(session.key(),session.encode().unwrap());
    let packages:BTreeMap<_,_>=mods.manager.packages.iter().filter(|(_,p)|p.running())
        .map(|(id,p)|(id.clone(),p.content_fingerprint())).collect();
    let local_ids:BTreeSet<_>=mods.bodies.iter().filter(|((o,_),_)|!o.starts_with('@')).map(|(_,id)|*id).collect();
    state.definitions.retain(|id,_|local_ids.contains(id));
    let stamp=state.started.elapsed().as_millis() as u64;
    for ((package,key),&id) in mods.bodies.iter().filter(|((o,_),_)|!o.starts_with('@')) {
        let Some(&fp)=packages.get(package) else {continue;};
        let Some(revision)=mods.world.definition_revision(id) else {continue;};
        if state.definitions.get(&id).is_none_or(|d|d.revision!=revision) {
            let Some(definition)=mods.world.body_definition(id) else {continue;};
            let bytes=match skate_dynamics::definition_codec::encode(&definition) {
                Ok(bytes)=>bytes,
                Err(error)=>{
                    issue(state,format!("{package}/{key}: collider definition: {error}"));
                    state.definitions.insert(id,CachedDefinition {revision,hash:0,chunks:Vec::new()});
                    continue;
                }
            };
            let chunks=if bytes.len()<=MAX_BLOB {bytes.chunks(CHUNK).map(|c|c.to_vec()).collect()} else {
                issue(state,format!("{package}/{key}: collider definition exceeds {MAX_BLOB} bytes"));Vec::new()
            };
            state.definitions.insert(id,CachedDefinition {revision,hash:skate_net::hash(&bytes),chunks});
        }
        let Some(definition)=state.definitions.get(&id) else {continue;};
        if definition.chunks.is_empty() {continue;}
        let Some(body)=mods.world.read(id) else {continue;};
        let pose=BodyPose {p:body.position,q:body.rotation,v:body.linvel,w:body.angvel,
            com:mods.world.local_center(id).unwrap_or([0.;3]),stamp};
        if !pose.valid() {issue(state,format!("{package}/{key}: non-finite or out-of-range body pose"));continue;}
        let mut header=make(BODY,package,key,fp,state.epoch,id,serde_json::to_vec(&pose).unwrap());
        header.revision=definition.hash;header.count=definition.chunks.len() as u16;
        let mut packets=vec![header.clone()];
        for (index,chunk) in definition.chunks.iter().enumerate() {
            let mut part=header.clone();part.kind=DEFINITION;part.index=index as u16;part.payload=chunk.clone();packets.push(part);
        }
        push_group(&mut out,packets,state,&format!("{package}/{key}"));
    }
    for ((package,key),g) in mods.graphics.iter().filter(|((o,_),_)|!o.starts_with('@')) {
        let Some(&fp)=packages.get(package) else {continue;};
        let frame=GraphicRecord {definition:g.definition.clone(),transform:g.transform.clone(),visible:g.visible,
            body_instance:g.body.as_ref().and_then(|body|mods.bodies.get(&(package.clone(),body.clone())).copied())};
        let Ok(data)=serde_json::to_vec(&frame) else {continue;};
        let mut packets=vec![make(GRAPHIC,package,key,fp,state.epoch,g.serial,data)];
        for (node,frame) in &g.nodes {
            let Ok(data)=serde_json::to_vec(&frame.state) else {continue;};
            let mut packet=make(NODE,package,key,fp,state.epoch,g.serial,data);packet.node=node.clone();packets.push(packet);
        }
        push_group(&mut out,packets,state,&format!("{package}/{key} graphics"));
    }
    if let Some(a)=&mods.attach {
        if let (Some(&fp),Some(&id))=(packages.get(&a.owner),mods.bodies.get(&(a.owner.clone(),a.body.clone()))) {
            let record=AttachmentRecord {offset:a.offset.to_array(),hidden:true};
            let p=make(ATTACH,&a.owner,&a.body,fp,state.epoch,id,serde_json::to_vec(&record).unwrap());
            push_group(&mut out,vec![p],state,"actor attachment");
        }
    }
    out
}

fn remove_body(mods:&mut Mods,state:&mut State,slot:&Slot) {
    if let Some(replica)=state.bodies.remove(slot) {
        let key=(owner(slot.0,&slot.1),slot.2.clone());
        if mods.bodies.get(&key)==Some(&replica.id) {mods.bodies.remove(&key);}
        mods.world.remove(replica.id);
    }
}
fn clear_remote(world:&mut World,mods:&mut Mods,state:&mut State) {
    for slot in state.bodies.keys().cloned().collect::<Vec<_>>() {remove_body(mods,state,&slot);}
    for slot in state.graphics.keys() {super::retire_graphics(world,mods,&(owner(slot.0,&slot.1),slot.2.clone()));}
    state.graphics.clear();state.node_sequences.clear();state.attachments.clear();
}

fn incoming(world:&mut World,mods:&mut Mods,state:&mut State,records:Vec<(u64,String,u32,Vec<u8>)>) {
    let packets:Vec<_>=records.iter().filter(|(_,key,_,value)|key.starts_with("m3:") && !value.is_empty())
        .filter_map(|(peer,key,seq,value)|Envelope::decode(key,value).map(|e|(*peer,*seq,e))).collect();
    let sessions:BTreeMap<_,_>=packets.iter().filter(|(_,_,e)|e.kind==SESSION).map(|(peer,_,e)|(*peer,e.epoch)).collect();
    if records.iter().any(|(_,key,_,data)|(key.starts_with("dyn:") || key.starts_with("m2:")) && !data.is_empty()) {
        issue(state,"A peer uses an older mod-object protocol; all players need the Model Collision Repair engine update".into());
    }
    let packages:BTreeMap<_,_>=mods.manager.packages.iter().filter(|(_,p)|p.running())
        .map(|(id,p)|(id.clone(),p.content_fingerprint())).collect();
    let accepted:Vec<_>=packets.iter().filter(|(peer,_,e)| {
        if e.kind==SESSION || sessions.get(peer)!=Some(&e.epoch) {return false;}
        if packages.get(&e.owner)!=Some(&e.fingerprint) {
            issue(state,format!("peer {peer}: package '{}' missing, disabled, or fingerprint differs",e.owner));return false;
        }
        true
    }).collect();
    let mut live_bodies=BTreeSet::new();let mut body_count=BTreeMap::<u64,usize>::new();
    for &&(peer,seq,ref packet) in &accepted {
        if packet.kind!=BODY {continue;}
        let count=body_count.entry(peer).or_default();if *count>=MAX_REMOTE_BODIES {continue;}*count+=1;
        let Ok(pose)=serde_json::from_slice::<BodyPose>(&packet.payload) else {continue;};
        if !pose.valid() {continue;}
        let slot=(peer,packet.owner.clone(),packet.key.clone());live_bodies.insert(slot.clone());
        let changed=state.bodies.get(&slot).is_none_or(|r|r.epoch!=packet.epoch || r.instance!=packet.instance || r.revision!=packet.revision);
        if changed {
            remove_body(mods,state,&slot);
            let Some(bytes)=assemble(packet,accepted.iter().filter(|r|r.0==peer).map(|r|&r.2)) else {continue;};
            let definition=match skate_dynamics::definition_codec::decode(&bytes) {
                Ok(definition)=>definition,
                Err(error)=>{issue(state,format!("{}: remote collider definition: {error}",packet.owner));continue;}
            };
            if !(0.01..=10_000_000.).contains(&definition.body.mass) {continue;}
            match mods.world.spawn_replica(&definition) {
                Ok(id) => {
                    mods.world.set_pose(id,pose.p,pose.q);
                    mods.world.set_kinematic_motion(id,pose.v,pose.w);
                    mods.bodies.insert((owner(peer,&packet.owner),packet.key.clone()),id);
                    state.bodies.insert(slot.clone(),Replica {id,epoch:packet.epoch,instance:packet.instance,revision:packet.revision,
                        seq,pose,received:Instant::now(),error_position:Vec3::ZERO,error_axis:Vec3::ZERO});
                },
                Err(error) => issue(state,format!("peer {peer} body {}/{}: {error}",packet.owner,packet.key)),
            }
        } else if let Some(replica)=state.bodies.get_mut(&slot) {replica.receive(pose,seq);}
    }
    let stale:Vec<_>=state.bodies.keys().filter(|slot|!live_bodies.contains(*slot)).cloned().collect();
    for slot in stale {remove_body(mods,state,&slot);}

    let mut live_graphics=BTreeSet::new();let mut graphics_count=BTreeMap::<u64,usize>::new();
    for &&(peer,_,ref packet) in &accepted {
        if packet.kind!=GRAPHIC {continue;}
        let count=graphics_count.entry(peer).or_default();if *count>=MAX_REMOTE_GRAPHICS {continue;}*count+=1;
        let Ok(record)=serde_json::from_slice::<GraphicRecord>(&packet.payload) else {continue;};
        if !record.definition.validate() || !record.transform.validate() {continue;}
        let slot=(peer,packet.owner.clone(),packet.key.clone());
        let o=owner(peer,&packet.owner);let key=(o.clone(),packet.key.clone());
        let bound=record.definition.body.as_ref().is_none_or(|body| {
            state.bodies.get(&(peer,packet.owner.clone(),body.clone())).is_some_and(|r|r.epoch==packet.epoch && Some(r.instance)==record.body_instance)
        });
        let visible=record.visible && bound;
        if state.graphics.get(&slot)!=Some(&(packet.epoch,packet.instance))
            || mods.graphics.get(&key).is_none_or(|g|g.definition!=record.definition) {
            if let Err(error)=graphics::spawn(world,mods,&o,&packet.owner,packet.key.clone(),record.definition,record.transform,visible,Some(packet.instance)) {
                issue(state,format!("peer {peer} scene {}/{}: {error}",packet.owner,packet.key));continue;
            }
            state.graphics.insert(slot.clone(),(packet.epoch,packet.instance));
        } else if let Some(g)=mods.graphics.get_mut(&key) {g.transform=record.transform;g.visible=visible;}
        live_graphics.insert(slot);
    }
    for slot in state.graphics.keys().filter(|s|!live_graphics.contains(*s)).cloned().collect::<Vec<_>>() {
        super::retire_graphics(world,mods,&(owner(slot.0,&slot.1),slot.2.clone()));state.graphics.remove(&slot);
    }
    let mut live_nodes=BTreeSet::new();let mut node_count=BTreeMap::<u64,usize>::new();
    for &&(peer,seq,ref packet) in &accepted {
        if packet.kind!=NODE {continue;}
        let count=node_count.entry(peer).or_default();if *count>=MAX_REMOTE_NODES {continue;}*count+=1;
        if state.graphics.get(&(peer,packet.owner.clone(),packet.key.clone()))!=Some(&(packet.epoch,packet.instance)) {continue;}
        let Ok(node)=serde_json::from_slice::<NodeState>(&packet.payload) else {continue;};if !node.validate() {continue;}
        let o=owner(peer,&packet.owner);let slot=(o.clone(),packet.key.clone(),packet.node.clone());
        live_nodes.insert(slot.clone());
        let Some(g)=mods.graphics.get_mut(&(o,packet.key.clone())) else {continue;};
        if state.node_sequences.get(&slot)!=Some(&(packet.epoch,packet.instance,seq)) || !g.nodes.contains_key(&packet.node) {
            g.nodes.insert(packet.node.clone(),graphics::TimedNode {state:node,received:Instant::now()});
            state.node_sequences.insert(slot,(packet.epoch,packet.instance,seq));
        }
    }
    for (o,key,node) in state.node_sequences.keys().filter(|s|!live_nodes.contains(*s)).cloned().collect::<Vec<_>>() {
        graphics::reset_node(world,mods,&o,&key,&node);state.node_sequences.remove(&(o,key,node));
    }
    state.attachments.clear();
    for &&(peer,_,ref packet) in &accepted {
        if packet.kind!=ATTACH {continue;}
        let Ok(record)=serde_json::from_slice::<AttachmentRecord>(&packet.payload) else {continue;};
        if !valid_vector(&record.offset,100.) {continue;}
        if state.bodies.get(&(peer,packet.owner.clone(),packet.key.clone())).is_some_and(|r|r.epoch==packet.epoch && r.instance==packet.instance) {
            state.attachments.insert(peer,Attachment {owner:packet.owner.clone(),key:packet.key.clone(),epoch:packet.epoch,instance:packet.instance,record});
        }
    }
}
fn sample(mods:&mut Mods,state:&State) {
    for replica in state.bodies.values() {
        let (t,v,w)=replica.sample();
        mods.world.set_pose(replica.id,t.translation.to_array(),t.rotation.to_array());
        mods.world.set_kinematic_motion(replica.id,v.to_array(),w.to_array());
    }
}
pub(crate) fn sample_fixed(world:&mut World) {
    world.resource_scope(|_world,mut mods:Mut<Mods>| {
        let state=std::mem::take(&mut mods.replication);sample(&mut mods,&state);mods.replication=state;
    });
}

pub(crate) fn sync(world:&mut World) {
    let (active,local,records)=world.get_resource::<crate::multiplayer::Multiplayer>()
        .map(|n|(n.active(),n.mod_identity().1,n.application_records())).unwrap_or((false,0,Vec::new()));
    world.resource_scope(|world,mut mods:Mut<Mods>| {
        let mut state=std::mem::take(&mut mods.replication);
        if !active {
            clear_remote(world,&mut mods,&mut state);state.connection=0;mods.dyn_published.clear();
            state.status="Solid replication offline".into();mods.replication=state;return;
        }
        if state.connection!=local {
            clear_remote(world,&mut mods,&mut state);state=State::default();state.connection=local;mods.dyn_published.clear();
        }
        if state.last_send.elapsed()>=Duration::from_millis(49) {
            let desired=outgoing(&mods,&mut state);
            let mut net=world.resource_mut::<crate::multiplayer::Multiplayer>();
            let mut published=BTreeSet::new();
            for (key,value) in desired {
                if net.publish_application(&key,value) {published.insert(key);}
                else {issue(&mut state,format!("transport rejected '{key}': application key/value budget reached"));}
            }
            for key in mods.dyn_published.difference(&published) {net.publish_application(key,Vec::new());}
            mods.dyn_published=published;state.last_send=Instant::now();
        }
        incoming(world,&mut mods,&mut state,records);
        sample(&mut mods,&state);
        graphics::sync(world,&mut mods);
        state.status=format!("Solid bridge: {} remote bodies / {} scenes{}",state.bodies.len(),state.graphics.len(),
            if state.last_problem.is_empty() {String::new()} else {format!(" | last issue: {}",state.last_problem)});
        mods.replication=state;
    });
}

pub(super) fn reset(world:&mut World,mods:&mut Mods) {
    if let Some(mut net)=world.get_resource_mut::<crate::multiplayer::Multiplayer>() {
        for key in &mods.dyn_published {net.publish_application(key,Vec::new());}
    }
    let mut old=std::mem::take(&mut mods.replication);clear_remote(world,mods,&mut old);
    mods.dyn_published.clear();
}
pub(crate) fn attached_root(mods:&Mods,peer:u64) -> Option<(Transform,bool)> {
    let attachment=mods.replication.attachments.get(&peer)?;
    let replica=mods.replication.bodies.get(&(peer,attachment.owner.clone(),attachment.key.clone()))?;
    if replica.epoch!=attachment.epoch || replica.instance!=attachment.instance {return None;}
    let snap=mods.world.read(replica.id)?;
    let q=Quat::from_array(snap.rotation).normalize();
    Some((Transform::from_translation(Vec3::from_array(snap.position)+q*Vec3::from_array(attachment.record.offset)).with_rotation(q),attachment.record.hidden))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rotating_offset_com_preserves_center_motion() {
        let pose=BodyPose {p:[1.,2.,3.],q:[0.,0.,0.,1.],v:[0.;3],w:[0.,2.,0.],com:[1.,0.,0.],stamp:0};
        let replica=Replica {id:1,epoch:1,instance:1,revision:0,seq:1,pose,received:Instant::now()-Duration::from_millis(50),error_position:Vec3::ZERO,error_axis:Vec3::ZERO};
        let (t,_,_)=replica.sample();let com=t.translation+t.rotation*Vec3::X;
        assert!(com.distance(Vec3::new(2.,2.,3.))<1e-4);
    }
    #[test] fn stale_owner_pose_does_not_extrapolate_forever() {
        let pose=BodyPose {p:[0.;3],q:[0.,0.,0.,1.],v:[100.,0.,0.],w:[0.;3],com:[0.;3],stamp:0};
        let replica=Replica {id:1,epoch:1,instance:1,revision:0,seq:1,pose,received:Instant::now()-Duration::from_secs(1),error_position:Vec3::ZERO,error_axis:Vec3::ZERO};
        let (t,v,w)=replica.sample();assert!((t.translation.x-10.).abs()<1e-5);assert_eq!(v,Vec3::ZERO);assert_eq!(w,Vec3::ZERO);
    }
}
