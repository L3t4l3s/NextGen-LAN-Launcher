//! Who wrote an event, and who may read a private one.
//!
//! A launcher's peer id is its Ed25519 public key (hex). Every event carries
//! a signature over its content, so nobody can write, delete or vote under
//! another launcher's id — only pick the same nickname. The body of a
//! private event is sealed with a NaCl box between the two Curve25519 keys
//! derived from the participants' ids: whoever else receives it (a forged
//! `Hello`, a misrouted frame) gets bytes it cannot open. What stays visible
//! is the envelope: who wrote to whom, when, under which nickname.

use super::model::{Body, Event};
use crypto_box::aead::Aead;
use crypto_box::{Nonce, PublicKey, SalsaBox, SecretKey};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngExt;

pub struct Keys {
    signing: SigningKey,
    id: String,
}

fn verifying_key(id: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = hex::decode(id).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// The bytes a signature covers: everything but the signature.
fn signed_bytes(e: &Event) -> Vec<u8> {
    serde_json::to_vec(&(&e.id, &e.from, &e.nick, e.seq, e.ts, &e.to, &e.body)).unwrap_or_default()
}

/// The author's signature is valid for this exact event.
pub fn verify(e: &Event) -> bool {
    let Some(key) = verifying_key(&e.from) else {
        return false;
    };
    let Some(sig) = hex::decode(&e.sig)
        .ok()
        .and_then(|b| <[u8; 64]>::try_from(b).ok())
    else {
        return false;
    };
    key.verify_strict(&signed_bytes(e), &Signature::from_bytes(&sig))
        .is_ok()
}

impl Keys {
    pub fn generate() -> Self {
        Self::from_secret(rand::rng().random::<[u8; 32]>())
    }

    pub fn from_secret(secret: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&secret);
        let id = hex::encode(signing.verifying_key().to_bytes());
        Self { signing, id }
    }

    pub fn from_secret_hex(text: &str) -> Option<Self> {
        let bytes: [u8; 32] = hex::decode(text.trim()).ok()?.try_into().ok()?;
        Some(Self::from_secret(bytes))
    }

    pub fn secret_hex(&self) -> String {
        hex::encode(self.signing.to_bytes())
    }

    /// The peer id: the public key in hex.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The box shared with `peer`; the same on both sides.
    fn shared_box(&self, peer: &str) -> Option<SalsaBox> {
        let theirs = PublicKey::from(verifying_key(peer)?.to_montgomery().to_bytes());
        let mine = SecretKey::from(self.signing.to_scalar_bytes());
        Some(SalsaBox::new(&theirs, &mine))
    }

    /// What goes on the wire for an event of our own: private bodies sealed
    /// for the recipient, everything signed.
    pub fn seal(&self, plain: &Event) -> Option<Event> {
        let mut wire = plain.clone();
        if let Some(to) = &plain.to {
            let nonce_bytes = rand::rng().random::<[u8; 24]>();
            let data = self
                .shared_box(to)?
                .encrypt(
                    Nonce::from_slice(&nonce_bytes),
                    serde_json::to_vec(&plain.body).ok()?.as_slice(),
                )
                .ok()?;
            wire.body = Body::Sealed {
                nonce: hex::encode(nonce_bytes),
                data: hex::encode(data),
            };
        }
        wire.sig = String::new();
        wire.sig = hex::encode(self.signing.sign(&signed_bytes(&wire)).to_bytes());
        Some(wire)
    }

    /// The readable event behind a wire event, or `None` when its signature
    /// does not hold or it is a private event this launcher is no part of.
    pub fn open(&self, wire: &Event) -> Option<Event> {
        if !verify(wire) {
            return None;
        }
        let mut plain = wire.clone();
        match (&wire.body, &wire.to) {
            (Body::Sealed { nonce, data }, Some(to)) => {
                let other = if wire.from == self.id {
                    to
                } else if *to == self.id {
                    &wire.from
                } else {
                    return None;
                };
                let nonce: [u8; 24] = hex::decode(nonce).ok()?.try_into().ok()?;
                let bytes = self
                    .shared_box(other)?
                    .decrypt(
                        Nonce::from_slice(&nonce),
                        hex::decode(data).ok()?.as_slice(),
                    )
                    .ok()?;
                plain.body = serde_json::from_slice(&bytes).ok()?;
                // A sealed body inside a sealed body would never be shown.
                if matches!(plain.body, Body::Sealed { .. }) {
                    return None;
                }
                Some(plain)
            }
            // Private but readable, or public but sealed: neither is ours.
            (Body::Sealed { .. }, None) | (_, Some(_)) => None,
            (_, None) => Some(plain),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(keys: &Keys, seq: u64, to: Option<&str>, text: &str) -> Event {
        Event {
            id: format!("{}:{seq}", keys.id()),
            from: keys.id().to_string(),
            nick: "Nick".into(),
            seq,
            ts: 1,
            to: to.map(String::from),
            body: Body::Text {
                text: text.into(),
                reply_to: None,
            },
            sig: String::new(),
        }
    }

    #[test]
    fn public_events_are_signed_and_tampering_shows() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let wire = alice.seal(&event(&alice, 1, None, "hi")).unwrap();
        assert_eq!(
            bob.open(&wire).unwrap().body,
            event(&alice, 1, None, "hi").body
        );
        let mut changed = wire.clone();
        changed.nick = "Mallory".into();
        assert!(bob.open(&changed).is_none());
        // Bob cannot write under Alice's id.
        let mut forged = event(&alice, 2, None, "delete everything");
        forged.sig = bob.seal(&forged).unwrap().sig;
        assert!(bob.open(&forged).is_none());
    }

    #[test]
    fn private_bodies_open_only_for_the_two_of_them() {
        let alice = Keys::generate();
        let bob = Keys::generate();
        let eve = Keys::generate();
        let plain = event(&alice, 1, Some(bob.id()), "psst");
        let wire = alice.seal(&plain).unwrap();
        assert!(matches!(wire.body, Body::Sealed { .. }));
        assert!(!serde_json::to_string(&wire).unwrap().contains("psst"));
        assert_eq!(bob.open(&wire).unwrap().body, plain.body);
        assert_eq!(alice.open(&wire).unwrap().body, plain.body, "own history");
        assert!(eve.open(&wire).is_none());
    }

    #[test]
    fn keys_survive_a_round_trip() {
        let keys = Keys::generate();
        let again = Keys::from_secret_hex(&keys.secret_hex()).unwrap();
        assert_eq!(again.id(), keys.id());
        assert_eq!(keys.id().len(), 64);
    }
}
