//! Bounded v3 wire framing over the existing owner-authenticated APPLICATION
//! stream. Geometry fragments are immutable; fast pose updates never restart
//! reassembly. Stable logical keys prevent respawns consuming more key slots.
use skate_mods::scene::{valid_key,valid_node};
pub(super) const SESSION:u8=0;
pub(super) const BODY:u8=1;
pub(super) const DEFINITION:u8=2;
pub(super) const GRAPHIC:u8=3;
pub(super) const NODE:u8=4;
pub(super) const ATTACH:u8=5;
pub(super) const CHUNK:usize=640;
pub(super) const MAX_CHUNKS:usize=192;
pub(super) const MAX_BLOB:usize=CHUNK*MAX_CHUNKS;
const _: () = assert!(MAX_BLOB == skate_dynamics::definition_codec::MAX_DEFINITION_BYTES);
const MAGIC:&[u8;4]=b"MS3\x03";
#[derive(Clone,Debug)]
pub(super) struct Envelope {
    pub kind:u8,pub owner:String,pub key:String,pub node:String,
    pub fingerprint:u64,pub epoch:u64,pub instance:u64,pub revision:u64,
    pub index:u16,pub count:u16,pub payload:Vec<u8>,
}
impl Envelope {
    pub fn session(epoch:u64) -> Self {
        Self {kind:SESSION,owner:String::new(),key:String::new(),node:String::new(),
            fingerprint:0,epoch,instance:0,revision:0,index:0,count:0,payload:Vec::new()}
    }
    pub fn key(&self) -> String {
        if self.kind==SESSION { return "m3:session".into(); }
        if self.kind==ATTACH { return "m3:attach".into(); }
        let id=format!("{}\0{}\0{}",self.owner,self.key,self.node);
        format!("m3:{}:{:016x}:{:02x}",self.kind,skate_net::hash(id.as_bytes()),self.index)
    }
    pub fn valid(&self) -> bool {
        if self.epoch==0 || self.kind>ATTACH { return false; }
        if self.kind==SESSION {
            return self.owner.is_empty() && self.key.is_empty() && self.node.is_empty()
                && self.instance==0 && self.fingerprint==0 && self.revision==0
                && self.index==0 && self.count==0 && self.payload.is_empty();
        }
        if !valid_key(&self.owner) || !valid_key(&self.key) || self.instance==0 { return false; }
        if self.kind==NODE {
            if !valid_node(&self.node) {return false;}
        } else if !self.node.is_empty() { return false; }
        match self.kind {
            DEFINITION => self.count>0 && self.count as usize<=MAX_CHUNKS
                && self.index<self.count && !self.payload.is_empty() && self.payload.len()<=CHUNK,
            BODY => self.index==0 && self.count>0 && self.count as usize<=MAX_CHUNKS,
            _ => self.index==0 && self.count==0,
        }
    }
    pub fn encode(&self) -> Option<Vec<u8>> {
        if !self.valid() { return None; }
        let mut out=Vec::with_capacity(44+self.owner.len()+self.key.len()+self.node.len()+self.payload.len());
        out.extend(MAGIC);out.push(self.kind);
        out.extend([self.owner.len() as u8,self.key.len() as u8,self.node.len() as u8]);
        for v in [self.fingerprint,self.epoch,self.instance,self.revision] { out.extend(v.to_le_bytes()); }
        out.extend(self.index.to_le_bytes());out.extend(self.count.to_le_bytes());
        out.extend(self.owner.as_bytes());out.extend(self.key.as_bytes());out.extend(self.node.as_bytes());out.extend(&self.payload);
        (out.len()<=skate_net::lobby::MAX_APP_VALUE).then_some(out)
    }
    pub fn decode(wire_key:&str,data:&[u8]) -> Option<Self> {
        if data.len()<44 || data.len()>skate_net::lobby::MAX_APP_VALUE || &data[..4]!=MAGIC {return None;}
        let owner_len=data[5] as usize;let key_len=data[6] as usize;let node_len=data[7] as usize;
        if owner_len>64 || key_len>64 || node_len>120 {return None;}
        let u64_at=|i|Some(u64::from_le_bytes(data.get(i..i+8)?.try_into().ok()?));
        let mut at=44;
        let mut string=|len| {
            let text=std::str::from_utf8(data.get(at..at+len)?).ok()?.to_owned();at+=len;Some(text)
        };
        let owner=string(owner_len)?;let key=string(key_len)?;let node=string(node_len)?;
        let out=Self { kind:data[4],owner,key,node,fingerprint:u64_at(8)?,epoch:u64_at(16)?,
            instance:u64_at(24)?,revision:u64_at(32)?,index:u16::from_le_bytes(data[40..42].try_into().ok()?),
            count:u16::from_le_bytes(data[42..44].try_into().ok()?),payload:data.get(at..)?.to_vec() };
        (out.valid() && out.key()==wire_key).then_some(out)
    }
    pub fn same_definition(&self,other:&Self) -> bool {
        self.owner==other.owner && self.key==other.key && self.fingerprint==other.fingerprint
            && self.epoch==other.epoch && self.instance==other.instance && self.revision==other.revision && self.count==other.count
    }
}
/// Assemble only fragments named by a live body record. No partial geometry
/// enters physics, and a mismatched chunk cannot mix two reset generations.
pub(super) fn assemble<'a>(body:&Envelope,fragments:impl Iterator<Item=&'a Envelope>) -> Option<Vec<u8>> {
    if body.kind!=BODY || body.count==0 || body.count as usize>MAX_CHUNKS { return None; }
    let mut pieces=vec![None;body.count as usize];
    for fragment in fragments {
        if fragment.kind!=DEFINITION || !fragment.same_definition(body) { continue; }
        let slot=pieces.get_mut(fragment.index as usize)?;
        if slot.is_some() { return None; }
        *slot=Some(fragment.payload.as_slice());
    }
    let mut bytes=Vec::new();
    for (index,piece) in pieces.into_iter().enumerate() {
        let piece=piece?;
        if index+1<body.count as usize && piece.len()!=CHUNK {return None;}
        if bytes.len()+piece.len()>MAX_BLOB {return None;}
        bytes.extend(piece);
    }
    (skate_net::hash(&bytes)==body.revision).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope() -> Envelope {
        Envelope {kind:BODY,owner:"mod".into(),key:"object".into(),node:String::new(),
            fingerprint:7,epoch:8,instance:9,revision:10,index:0,count:1,payload:vec![1,2,3]}
    }
    #[test] fn exact_key_and_round_trip() {
        let e=envelope();let bytes=e.encode().unwrap();
        assert_eq!(Envelope::decode(&e.key(),&bytes).unwrap().payload,e.payload);
        assert!(Envelope::decode("m3:wrong",&bytes).is_none());
        for n in 0..44 {assert!(Envelope::decode(&e.key(),&bytes[..n]).is_none());}
    }
    #[test] fn reassembly_requires_all_matching_fragments() {
        let data=vec![42;CHUNK+7];let mut body=envelope();body.count=2;body.revision=skate_net::hash(&data);
        let mut a=body.clone();a.kind=DEFINITION;a.payload=data[..CHUNK].to_vec();
        let mut b=a.clone();b.index=1;b.payload=data[CHUNK..].to_vec();
        assert!(assemble(&body,[&a].into_iter()).is_none());
        assert_eq!(assemble(&body,[&b,&a].into_iter()).unwrap(),data);
        b.instance+=1;assert!(assemble(&body,[&a,&b].into_iter()).is_none());
    }
    #[test] fn limits_are_enforced_before_allocation() {
        let mut e=envelope();e.count=MAX_CHUNKS as u16+1;assert!(e.encode().is_none());
        e.count=1;e.payload=vec![0;1024];assert!(e.encode().is_none());
        e.payload.clear();e.owner="../other".into();assert!(e.encode().is_none());
    }
}
