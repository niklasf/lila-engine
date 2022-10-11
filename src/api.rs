use std::{cmp::min, fmt, num::NonZeroU32};

use rand::{
    distributions::{Alphanumeric, DistString},
    thread_rng,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr, TryFromInto};
use sha2::{Digest, Sha256};
use shakmaty::{
    fen::Fen,
    uci::{IllegalUciError, Uci},
    variant::{Variant, VariantPosition},
    CastlingMode, EnPassantMode, Position as _, PositionError,
};
use thiserror::Error;

use crate::repo::ExternalEngine;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct UserId(String);

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SessionId(String);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EngineId(pub String);

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct MultiPv(u32);

impl Default for MultiPv {
    fn default() -> MultiPv {
        MultiPv(1)
    }
}

#[derive(Error, Debug)]
#[error("supported range is 1 to 5")]
pub struct InvalidMultiPvError;

impl TryFrom<u32> for MultiPv {
    type Error = InvalidMultiPvError;

    fn try_from(n: u32) -> Result<MultiPv, InvalidMultiPvError> {
        if 1 <= n && n <= 5 {
            Ok(MultiPv(n))
        } else {
            Err(InvalidMultiPvError)
        }
    }
}

impl From<MultiPv> for u32 {
    fn from(MultiPv(n): MultiPv) -> u32 {
        n
    }
}

impl From<MultiPv> for usize {
    fn from(MultiPv(n): MultiPv) -> usize {
        n as usize
    }
}

impl fmt::Display for MultiPv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Deserialize, Debug)]
pub struct ProviderSecret(String);

impl ProviderSecret {
    pub fn selector(&self) -> ProviderSelector {
        let mut hasher = Sha256::new();
        hasher.update("providerSecret:");
        hasher.update(self.0.as_bytes());
        ProviderSelector(hex::encode(hasher.finalize()))
    }
}

#[derive(Deserialize, Eq, PartialEq, Hash, Debug, Clone)]
pub struct ProviderSelector(String);

#[derive(Deserialize, Serialize, Debug, Eq, Clone)]
pub struct ClientSecret(String);

impl PartialEq for ClientSecret {
    fn eq(&self, other: &ClientSecret) -> bool {
        // Best effort constant time equality
        self.0.len() == other.0.len()
            && self
                .0
                .bytes()
                .zip(other.0.bytes())
                .fold(0, |acc, (left, right)| acc | (left ^ right))
                == 0
    }
}

#[derive(Debug, Deserialize, Serialize, Copy, Clone)]
pub enum LichessVariant {
    #[serde(alias = "antichess")]
    Antichess,
    #[serde(alias = "atomic")]
    Atomic,
    #[serde(alias = "chess960")]
    Chess960,
    #[serde(alias = "crazyhouse")]
    Crazyhouse,
    #[serde(alias = "fromPosition", alias = "From Position")]
    FromPosition,
    #[serde(alias = "horde")]
    Horde,
    #[serde(alias = "kingOfTheHill", alias = "King of the Hill")]
    KingOfTheHill,
    #[serde(alias = "racingKings", alias = "Racing Kings")]
    RacingKings,
    #[serde(alias = "chess", alias = "standard")]
    Standard,
    #[serde(alias = "threeCheck", alias = "Three-check")]
    ThreeCheck,
}

impl From<LichessVariant> for Variant {
    fn from(variant: LichessVariant) -> Variant {
        match variant {
            LichessVariant::Antichess => Variant::Antichess,
            LichessVariant::Atomic => Variant::Atomic,
            LichessVariant::Chess960 | LichessVariant::FromPosition | LichessVariant::Standard => {
                Variant::Chess
            }
            LichessVariant::Crazyhouse => Variant::Crazyhouse,
            LichessVariant::Horde => Variant::Horde,
            LichessVariant::KingOfTheHill => Variant::KingOfTheHill,
            LichessVariant::RacingKings => Variant::RacingKings,
            LichessVariant::ThreeCheck => Variant::ThreeCheck,
        }
    }
}

impl From<Variant> for LichessVariant {
    fn from(variant: Variant) -> LichessVariant {
        match variant {
            Variant::Chess => LichessVariant::Standard,
            Variant::Antichess => LichessVariant::Antichess,
            Variant::Atomic => LichessVariant::Atomic,
            Variant::Crazyhouse => LichessVariant::Crazyhouse,
            Variant::Horde => LichessVariant::Horde,
            Variant::KingOfTheHill => LichessVariant::KingOfTheHill,
            Variant::RacingKings => LichessVariant::RacingKings,
            Variant::ThreeCheck => LichessVariant::ThreeCheck,
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AnalyseRequest {
    pub client_secret: ClientSecret,
    pub work: Work,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobId(String);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl JobId {
    pub fn random() -> JobId {
        JobId(Alphanumeric.sample_string(&mut thread_rng(), 16))
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    session_id: SessionId,
    threads: NonZeroU32,
    hash: NonZeroU32,
    deep: bool,
    #[serde_as(as = "TryFromInto<u32>")]
    multi_pv: MultiPv,
    variant: LichessVariant,
    #[serde_as(as = "DisplayFromStr")]
    initial_fen: Fen,
    #[serde_as(as = "Vec<DisplayFromStr>")]
    moves: Vec<Uci>,
}

#[derive(Error, Debug)]
pub enum InvalidWorkError {
    #[error("illegal initial position: {0}")]
    Position(#[from] PositionError<VariantPosition>),
    #[error("illegal uci: {0}")]
    IllegalUci(#[from] IllegalUciError),
    #[error("too many moves")]
    TooManyMoves,
    #[error("unsupported variant")]
    UnsupportedVariant,
}

impl Work {
    pub fn sanitize(
        self,
        engine: &ExternalEngine,
    ) -> Result<(Work, VariantPosition), InvalidWorkError> {
        let variant = self.variant.into();
        if !engine
            .variants
            .iter()
            .copied()
            .any(|v| Variant::from(v) == variant)
        {
            return Err(InvalidWorkError::UnsupportedVariant);
        }
        let mut pos = VariantPosition::from_setup(
            variant,
            self.initial_fen.into_setup(),
            CastlingMode::Chess960,
        )?;
        let initial_fen = Fen(pos.clone().into_setup(EnPassantMode::Legal));
        if self.moves.len() > 600 {
            return Err(InvalidWorkError::TooManyMoves);
        }
        let mut moves = Vec::with_capacity(self.moves.len());
        for uci in self.moves {
            let m = uci.to_move(&pos)?;
            moves.push(m.to_uci(CastlingMode::Chess960));
            pos.play_unchecked(&m);
        }
        Ok((
            Work {
                session_id: self.session_id,
                threads: min(self.threads, engine.max_threads),
                hash: min(self.hash, engine.max_hash),
                deep: self.deep,
                multi_pv: self.multi_pv,
                variant: variant.into(),
                initial_fen,
                moves,
            },
            pos,
        ))
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AcquireRequest {
    pub provider_secret: ProviderSecret,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AcquireResponse {
    pub id: JobId,
    pub work: Work,
    pub engine: ExternalEngine,
}
