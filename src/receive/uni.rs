use std::sync::Arc;

use super::InputPair;
use crate::bitcoin_ffi::{Address, OutPoint, Script, TxOut};
use crate::error::ForeignError;
pub use crate::receive::{
    Error, ImplementationError, InputContributionError, JsonReply, OutputSubstitutionError,
    ReplyableError, SelectionError, SerdeJsonError, SessionError,
};
use crate::{ClientResponse, OhttpKeys, OutputSubstitution, Request};

macro_rules! impl_from_super_methods {
    ($uni_type:ty, $inner_type:ty) => {
        impl From<$inner_type> for $uni_type {
            fn from(value: $inner_type) -> Self {
                Self(value)
            }
        }

        impl From<$uni_type> for $inner_type {
            fn from(value: $uni_type) -> Self {
                value.0
            }
        }
    };
}

#[derive(uniffi::Object)]
pub struct MaybeBadInitInputsTransition(super::InitInputsTransition);

#[uniffi::export]
impl MaybeBadInitInputsTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<ReceiverWithContext, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        // Take ownership of the inner InitInputsTransition
        let counter = Arc::strong_count(&self);
        println!("counter: {}", counter);
        let inner = Arc::into_inner(self).unwrap();
        Ok(ReceiverWithContext(inner.0.save(&adapter)?))
    }
}

#[derive(uniffi::Object)]
pub struct UninitializedReceiver {}

#[uniffi::export]
impl UninitializedReceiver {
    #[uniffi::constructor]
    // TODO: no need for this constructor. `create_session` is the only way to create a receiver.
    pub fn new() -> Self {
        Self {}
    }

    pub fn create_session(
        &self,
        address: Arc<Address>,
        directory: String,
        ohttp_keys: Arc<OhttpKeys>,
        expire_after: Option<u64>,
    ) -> MaybeBadInitInputsTransition {
        MaybeBadInitInputsTransition(
            super::UninitializedReceiver::create_session(
                (*address).clone(),
                directory,
                (*ohttp_keys).clone(),
                expire_after,
            )
            .into(),
        )
    }
}

#[derive(uniffi::Object, Clone)]
pub struct ReceiverWithContext(super::ReceiverWithContext);

impl_from_super_methods!(ReceiverWithContext, super::ReceiverWithContext);

#[derive(uniffi::Object)]
pub struct MaybeFatalTransitionWithNoResults(
    Arc<
        payjoin::persist::MaybeFatalTransitionWithNoResults<
            UniReceiverSessionEvent,
            UncheckedProposal,
            ReceiverWithContext,
            Error,
        >,
    >,
);

impl
    From<
        payjoin::persist::MaybeFatalTransitionWithNoResults<
            UniReceiverSessionEvent,
            UncheckedProposal,
            ReceiverWithContext,
            Error,
        >,
    > for MaybeFatalTransitionWithNoResults
{
    fn from(
        value: payjoin::persist::MaybeFatalTransitionWithNoResults<
            UniReceiverSessionEvent,
            UncheckedProposal,
            ReceiverWithContext,
            Error,
        >,
    ) -> Self {
        Self(Arc::new(value))
    }
}

#[derive(uniffi::Object)]
pub struct ReceiverWithContextTransition(super::ReceiverWithContextTransition);

#[derive(uniffi::Object)]
pub struct ReceiverWithContextTransitionOutcome(super::ReceiverWithContextTransitionOutcome);

impl From<super::ReceiverWithContextTransitionOutcome> for ReceiverWithContextTransitionOutcome {
    fn from(value: super::ReceiverWithContextTransitionOutcome) -> Self {
        Self(value)
    }
}

impl ReceiverWithContextTransitionOutcome {
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    pub fn is_success(&self) -> bool {
        self.0.is_success()
    }

    pub fn success(&self) -> Option<UncheckedProposal> {
        self.0.success().map(|p| p.into())
    }
}

#[uniffi::export]
impl ReceiverWithContextTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<ReceiverWithContextTransitionOutcome, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self)
            .map_err(|_| {
                ImplementationError::from("Multiple references to transition object".to_string())
            })?
            .0;
        let res = inner.save(&adapter)?;
        Ok(ReceiverWithContextTransitionOutcome(res))
    }
}

#[uniffi::export]
impl ReceiverWithContext {
    pub fn extract_req(&self, ohttp_relay: String) -> Result<RequestResponse, Error> {
        self.0
            .extract_req(ohttp_relay)
            .map(|(request, ctx)| RequestResponse { request, client_response: Arc::new(ctx) })
    }

    pub fn process_res(
        // TODO: this should consume self
        &self,
        body: &[u8],
        context: Arc<ClientResponse>,
    ) -> ReceiverWithContextTransition {
        ReceiverWithContextTransition(self.0.process_res(body, &context))
    }

    pub fn pj_uri(&self) -> crate::PjUri {
        self.0.pj_uri().into()
    }
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RequestResponse {
    pub request: Request,
    pub client_response: Arc<ClientResponse>,
}

#[uniffi::export(with_foreign)]
pub trait CanBroadcast: Send + Sync {
    fn callback(&self, tx: Vec<u8>) -> Result<bool, ForeignError>;
}

/// The sender's original PSBT and optional parameters
///
/// This type is used to proces the request. It is returned by UncheckedProposal::from_request().
///
/// If you are implementing an interactive payment processor, you should get extract the original transaction with get_transaction_to_schedule_broadcast() and schedule, followed by checking that the transaction can be broadcast with check_can_broadcast. Otherwise it is safe to call assume_interactive_receive to proceed with validation.
#[derive(Clone, uniffi::Object)]
pub struct UncheckedProposal(super::UncheckedProposal);

impl_from_super_methods!(UncheckedProposal, super::UncheckedProposal);

#[derive(uniffi::Object)]
pub struct UncheckedProposalTransition(super::UncheckedProposalTransition);

impl UncheckedProposalTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<MaybeInputsOwned, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

#[uniffi::export]
impl UncheckedProposal {
    /// The Sender's Original PSBT
    pub fn extract_tx_to_schedule_broadcast(&self) -> Vec<u8> {
        self.0.extract_tx_to_schedule_broadcast()
    }

    /// Call after checking that the Original PSBT can be broadcast.
    ///
    /// Receiver MUST check that the Original PSBT from the sender can be broadcast, i.e. testmempoolaccept bitcoind rpc returns { "allowed": true,.. } for get_transaction_to_check_broadcast() before calling this method.
    ///
    /// Do this check if you generate bitcoin uri to receive Payjoin on sender request without manual human approval, like a payment processor. Such so called "non-interactive" receivers are otherwise vulnerable to probing attacks. If a sender can make requests at will, they can learn which bitcoin the receiver owns at no cost. Broadcasting the Original PSBT after some time in the failure case makes incurs sender cost and prevents probing.
    ///
    /// Call this after checking downstream.
    pub fn check_broadcast_suitability(
        &self,
        min_fee_rate: Option<u64>,
        can_broadcast: Arc<dyn CanBroadcast>,
    ) -> UncheckedProposalTransition {
        UncheckedProposalTransition(self.0.check_broadcast_suitability(
            min_fee_rate,
            |transaction| {
                can_broadcast
                    .callback(transaction.to_vec())
                    .map_err(|e| ImplementationError::from(e.to_string()))
            },
        ))
    }

    /// Call this method if the only way to initiate a Payjoin with this receiver
    /// requires manual intervention, as in most consumer wallets.
    ///
    /// So-called "non-interactive" receivers, like payment processors, that allow arbitrary requests are otherwise vulnerable to probing attacks.
    /// Those receivers call `extract_tx_to_check_broadcast()` and `attest_tested_and_scheduled_broadcast()` after making those checks downstream.
    pub fn assume_interactive_receiver(&self) -> Arc<MaybeInputsOwned> {
        todo!("Implement this")
        // Arc::new(self.0.assume_interactive_receiver().into())
    }

    /// Extract an OHTTP Encapsulated HTTP POST request to return
    /// a Receiver Error Response
    pub fn extract_err_req(
        &self,
        err: Arc<JsonReply>,
        ohttp_relay: String,
    ) -> Result<RequestResponse, SessionError> {
        self.0
            .extract_err_req(&err, ohttp_relay)
            .map(|(req, ctx)| RequestResponse { request: req, client_response: Arc::new(ctx) })
    }

    /// Process an OHTTP Encapsulated HTTP POST Error response
    /// to ensure it has been posted properly
    pub fn process_err_res(
        &self,
        body: &[u8],
        context: Arc<ClientResponse>,
    ) -> Result<(), SessionError> {
        self.0.clone().process_err_res(body, &context)
    }
}

/// Type state to validate that the Original PSBT has no receiver-owned inputs.
/// Call check_no_receiver_owned_inputs() to proceed.
#[derive(Clone, uniffi::Object)]
pub struct MaybeInputsOwned(super::MaybeInputsOwned);

impl_from_super_methods!(MaybeInputsOwned, super::MaybeInputsOwned);

#[uniffi::export(with_foreign)]
pub trait IsScriptOwned: Send + Sync {
    fn callback(&self, script: Vec<u8>) -> Result<bool, ForeignError>;
}

#[derive(uniffi::Object)]
pub struct MaybeInputsOwnedTransition(super::MaybeInputsOwnedTransition);

#[uniffi::export]
impl MaybeInputsOwnedTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<MaybeInputsSeen, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

#[uniffi::export]
impl MaybeInputsOwned {
    ///Check that the Original PSBT has no receiver-owned inputs. Return original-psbt-rejected error or otherwise refuse to sign undesirable inputs.
    /// An attacker could try to spend receiver's own inputs. This check prevents that.
    pub fn check_inputs_not_owned(
        &self,
        is_owned: Arc<dyn IsScriptOwned>,
    ) -> MaybeInputsOwnedTransition {
        MaybeInputsOwnedTransition(self.0.check_inputs_not_owned(|input| {
            is_owned.callback(input.to_vec()).map_err(|e| ImplementationError::from(e.to_string()))
        }))
    }
}

#[uniffi::export(with_foreign)]
pub trait IsOutputKnown: Send + Sync {
    fn callback(&self, outpoint: OutPoint) -> Result<bool, ForeignError>;
}

/// Typestate to validate that the Original PSBT has no inputs that have been seen before.
///
/// Call check_no_inputs_seen to proceed.
#[derive(Clone, uniffi::Object)]
pub struct MaybeInputsSeen(super::MaybeInputsSeen);

impl_from_super_methods!(MaybeInputsSeen, super::MaybeInputsSeen);

#[derive(uniffi::Object)]
pub struct MaybeInputsSeenTransition(super::MaybeInputsSeenTransition);

impl MaybeInputsSeenTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<OutputsUnknown, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}
#[uniffi::export]
impl MaybeInputsSeen {
    /// Make sure that the original transaction inputs have never been seen before. This prevents probing attacks. This prevents reentrant Payjoin, where a sender proposes a Payjoin PSBT as a new Original PSBT for a new Payjoin.
    pub fn check_no_inputs_seen_before(
        &self,
        is_known: Arc<dyn IsOutputKnown>,
    ) -> MaybeInputsSeenTransition {
        MaybeInputsSeenTransition(self.0.clone().check_no_inputs_seen_before(|outpoint| {
            is_known
                .callback(outpoint.clone())
                .map_err(|e| ImplementationError::from(e.to_string()))
        }))
    }
}

/// The receiver has not yet identified which outputs belong to the receiver.
///
/// Only accept PSBTs that send us money. Identify those outputs with identify_receiver_outputs() to proceed
#[derive(Clone, uniffi::Object)]
pub struct OutputsUnknown(super::OutputsUnknown);

impl_from_super_methods!(OutputsUnknown, super::OutputsUnknown);

#[derive(uniffi::Object)]
pub struct OutputsUnknownTransition(super::OutputsUnknownTransition);

#[uniffi::export]
impl OutputsUnknownTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<WantsOutputs, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

impl OutputsUnknown {
    /// Find which outputs belong to the receiver
    pub fn identify_receiver_outputs(
        &self,
        is_receiver_output: Arc<dyn IsScriptOwned>,
    ) -> OutputsUnknownTransition {
        OutputsUnknownTransition(self.0.clone().identify_receiver_outputs(|output_script| {
            is_receiver_output
                .callback(output_script.to_vec())
                .map_err(|e| ImplementationError::from(e.to_string()))
        }))
    }
}

#[derive(uniffi::Object)]
pub struct WantsOutputs(super::WantsOutputs);

impl_from_super_methods!(WantsOutputs, super::WantsOutputs);

#[derive(uniffi::Object)]
pub struct WantsOutputsTransition(super::WantsOutputsTransition);

impl WantsOutputsTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<WantsInputs, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

#[uniffi::export]
impl WantsOutputs {
    pub fn output_substitution(&self) -> OutputSubstitution {
        self.0.output_substitution()
    }

    pub fn replace_receiver_outputs(
        &self,
        replacement_outputs: Vec<TxOut>,
        drain_script: Arc<Script>,
    ) -> Result<Arc<WantsOutputs>, OutputSubstitutionError> {
        self.0
            .replace_receiver_outputs(replacement_outputs, &drain_script)
            .map(|t| Arc::new(t.into()))
    }

    pub fn commit_outputs(&self) -> WantsOutputsTransition {
        WantsOutputsTransition(self.0.commit_outputs())
    }

    pub fn substitute_receiver_script(
        &self,
        output_script: Arc<Script>,
    ) -> Result<Arc<WantsOutputs>, OutputSubstitutionError> {
        self.0.substitute_receiver_script(&output_script).map(|t| Arc::new(t.into()))
    }
}

#[derive(uniffi::Object)]
pub struct WantsInputs(super::WantsInputs);

impl_from_super_methods!(WantsInputs, super::WantsInputs);

#[derive(uniffi::Object)]
pub struct WantsInputsTransition(super::WantsInputsTransition);

impl WantsInputsTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<ProvisionalProposal, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

#[uniffi::export]
impl WantsInputs {
    /// Select receiver input such that the payjoin avoids surveillance.
    /// Return the input chosen that has been applied to the Proposal.
    ///
    /// Proper coin selection allows payjoin to resemble ordinary transactions.
    /// To ensure the resemblance, a number of heuristics must be avoided.
    ///
    /// UIH "Unnecessary input heuristic" is one class of them to avoid. We define
    /// UIH1 and UIH2 according to the BlockSci practice
    /// BlockSci UIH1 and UIH2:
    // if min(out) < min(in) then UIH1 else UIH2
    // https://eprint.iacr.org/2022/589.pdf
    pub fn try_preserving_privacy(
        &self,
        candidate_inputs: Vec<Arc<InputPair>>,
    ) -> Result<Arc<InputPair>, SelectionError> {
        let candidate_inputs: Vec<InputPair> = candidate_inputs
            .into_iter()
            .map(|pair| Arc::try_unwrap(pair).unwrap_or_else(|arc| (*arc).clone()))
            .collect();

        self.0.try_preserving_privacy(candidate_inputs).map(Arc::new)
    }

    pub fn contribute_inputs(
        &self,
        replacement_inputs: Vec<Arc<InputPair>>,
    ) -> Result<Arc<WantsInputs>, InputContributionError> {
        let replacement_inputs: Vec<InputPair> = replacement_inputs
            .into_iter()
            .map(|pair| Arc::try_unwrap(pair).unwrap_or_else(|arc| (*arc).clone()))
            .collect();
        self.0.contribute_inputs(replacement_inputs).map(|t| Arc::new(t.into()))
    }

    pub fn commit_inputs(&self) -> WantsInputsTransition {
        WantsInputsTransition(self.0.commit_inputs())
    }
}

#[derive(uniffi::Object)]
pub struct ProvisionalProposal(super::ProvisionalProposal);

impl_from_super_methods!(ProvisionalProposal, super::ProvisionalProposal);

#[derive(uniffi::Object)]
pub struct ProvisionalProposalTransition(super::ProvisionalProposalTransition);

impl ProvisionalProposalTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<PayjoinProposal, ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        let res = inner.0.save(&adapter)?;
        Ok(res.into())
    }
}

/// A mutable checked proposal that the receiver may contribute inputs to to make a payjoin.
#[uniffi::export]
impl ProvisionalProposal {
    pub fn finalize_proposal(
        &self,
        process_psbt: Arc<dyn ProcessPsbt>,
        min_feerate_sat_per_vb: Option<u64>,
        max_effective_fee_rate_sat_per_vb: Option<u64>,
    ) -> ProvisionalProposalTransition {
        ProvisionalProposalTransition(self.0.finalize_proposal(
            |psbt| {
                process_psbt
                    .callback(psbt.to_string())
                    .map_err(|e| ImplementationError::from(e.to_string()))
            },
            min_feerate_sat_per_vb,
            max_effective_fee_rate_sat_per_vb,
        ))
    }
}

#[uniffi::export(with_foreign)]
pub trait ProcessPsbt: Send + Sync {
    fn callback(&self, psbt: String) -> Result<String, ForeignError>;
}

#[derive(Clone, uniffi::Object)]
pub struct PayjoinProposal(super::PayjoinProposal);

impl_from_super_methods!(PayjoinProposal, super::PayjoinProposal);

#[derive(uniffi::Object)]
pub struct PayjoinProposalTransition(super::PayjoinProposalTransition);

impl PayjoinProposalTransition {
    pub fn save(
        self: Arc<Self>,
        persister: Arc<dyn JsonReceiverPersistedSession>,
    ) -> Result<(), ImplementationError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let inner = Arc::try_unwrap(self).map_err(|_| {
            ImplementationError::from("Multiple references to transition object".to_string())
        })?;
        inner.0.save(&adapter)?;
        Ok(())
    }
}

#[uniffi::export]
impl PayjoinProposal {
    pub fn utxos_to_be_locked(&self) -> Vec<crate::OutPoint> {
        let mut outpoints: Vec<crate::OutPoint> = Vec::new();
        for e in <PayjoinProposal as Into<super::PayjoinProposal>>::into(self.clone())
            .utxos_to_be_locked()
        {
            outpoints.push(e.to_owned());
        }
        outpoints
    }

    pub fn psbt(&self) -> String {
        self.0.psbt()
    }

    /// Extract an OHTTP Encapsulated HTTP POST request for the Proposal PSBT
    pub fn extract_req(&self, ohttp_relay: String) -> Result<RequestResponse, Error> {
        let (req, res) = self.0.extract_req(ohttp_relay)?;
        Ok(RequestResponse { request: req, client_response: Arc::new(res) })
    }

    ///Processes the response for the final POST message from the receiver client in the v2 Payjoin protocol.
    ///
    /// This function decapsulates the response using the provided OHTTP context. If the response status is successful, it indicates that the Payjoin proposal has been accepted. Otherwise, it returns an error with the status code.
    ///
    /// After this function is called, the receiver can either wait for the Payjoin transaction to be broadcast or choose to broadcast the original PSBT.
    pub fn process_res(&self, body: &[u8], ctx: Arc<ClientResponse>) -> PayjoinProposalTransition {
        PayjoinProposalTransition(self.0.process_res(body, ctx.as_ref()))
    }
}

/// A trait for a persisted session that can be used to save and load events as JSON strings.
#[uniffi::export(with_foreign)]
pub trait JsonReceiverPersistedSession: Send + Sync {
    fn save(&self, event: String) -> Result<(), ForeignError>;
    fn load(&self) -> Result<Vec<String>, ForeignError>;
    fn close(&self) -> Result<(), ForeignError>;
}

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum UniReceiverError {
    #[error("Some error")]
    SomeError(String),
}

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize)]
pub struct UniReceiverSessionEvent(super::ReceiverSessionEvent);

impl From<payjoin::receive::v2::ReceiverSessionEvent> for UniReceiverSessionEvent {
    fn from(value: payjoin::receive::v2::ReceiverSessionEvent) -> Self {
        UniReceiverSessionEvent(value.into())
    }
}

impl From<UniReceiverSessionEvent> for payjoin::receive::v2::ReceiverSessionEvent {
    fn from(value: UniReceiverSessionEvent) -> Self {
        value.0.into()
    }
}

#[uniffi::export]
impl UniReceiverSessionEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.0).unwrap()
    }

    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Self, UniReceiverError> {
        let event: payjoin::receive::v2::ReceiverSessionEvent =
            serde_json::from_str(&json).unwrap();
        Ok(UniReceiverSessionEvent(event.into()))
    }
}

impl From<UniReceiverSessionEvent> for super::ReceiverSessionEvent {
    fn from(value: UniReceiverSessionEvent) -> Self {
        value.0
    }
}

impl From<super::ReceiverSessionEvent> for UniReceiverSessionEvent {
    fn from(value: super::ReceiverSessionEvent) -> Self {
        UniReceiverSessionEvent(value)
    }
}

/// This is a representation of the receiver state that is used by the foreign language.
/// Each inner type is a stateful representation which hold a reference to the persister.
#[derive(Clone, uniffi::Enum)]
pub enum UniReceiverState {
    Uninitialized,
    WithContext { inner: Arc<ReceiverWithContext> },
    UncheckedProposal { inner: Arc<UncheckedProposal> },
    MaybeInputsOwned { inner: Arc<MaybeInputsOwned> },
    MaybeInputsSeen { inner: Arc<MaybeInputsSeen> },
    OutputsUnknown { inner: Arc<OutputsUnknown> },
    WantsOutputs { inner: Arc<WantsOutputs> },
    WantsInputs { inner: Arc<WantsInputs> },
    ProvisionalProposal { inner: Arc<ProvisionalProposal> },
    PayjoinProposal { inner: Arc<PayjoinProposal> },
    FallbackBroadcasted { txid: String },
    SessionInvalid { reason: String },
}

#[derive(uniffi::Object)]
pub struct SessionHistory(std::sync::Mutex<super::SessionHistory>);

impl From<super::SessionHistory> for SessionHistory {
    fn from(value: super::SessionHistory) -> Self {
        Self(std::sync::Mutex::new(value))
    }
}

impl From<SessionHistory> for super::SessionHistory {
    fn from(value: SessionHistory) -> Self {
        value.0.into_inner().unwrap()
    }
}

impl From<super::ReceiverState> for UniReceiverState {
    fn from(value: super::ReceiverState) -> Self {
        match value.0 {
            payjoin::receive::v2::ReceiverState::Uninitialized(_) => Self::Uninitialized,
            payjoin::receive::v2::ReceiverState::WithContext(inner) => {
                Self::WithContext {
                    inner: Arc::new(super::ReceiverWithContext::from(inner).into()),
                }
            }
            payjoin::receive::v2::ReceiverState::UncheckedProposal(inner) => {
                Self::UncheckedProposal {
                    inner: Arc::new(super::UncheckedProposal::from(inner).into()),
                }
            }
            payjoin::receive::v2::ReceiverState::MaybeInputsOwned(inner) => {
                Self::MaybeInputsOwned {
                    inner: Arc::new(super::MaybeInputsOwned::from(inner).into()),
                }
            }
            payjoin::receive::v2::ReceiverState::MaybeInputsSeen(inner) => {
                Self::MaybeInputsSeen {
                    inner: Arc::new(super::MaybeInputsSeen::from(inner).into()),
                }
            }
            payjoin::receive::v2::ReceiverState::OutputsUnknown(inner) => {
                Self::OutputsUnknown { inner: Arc::new(super::OutputsUnknown::from(inner).into()) }
            }
            payjoin::receive::v2::ReceiverState::WantsOutputs(inner) => {
                Self::WantsOutputs { inner: Arc::new(super::WantsOutputs::from(inner).into()) }
            }
            payjoin::receive::v2::ReceiverState::WantsInputs(inner) => {
                Self::WantsInputs { inner: Arc::new(super::WantsInputs::from(inner).into()) }
            }
            payjoin::receive::v2::ReceiverState::ProvisionalProposal(inner) => {
                Self::ProvisionalProposal {
                    inner: Arc::new(super::ProvisionalProposal::from(inner).into()),
                }
            }
            payjoin::receive::v2::ReceiverState::PayjoinProposal(inner) => {
                Self::PayjoinProposal {
                    inner: Arc::new(super::PayjoinProposal::from(inner).into()),
                }
            }
            _ => todo!("Implement remaining receiver state conversions"),
        }
    }
}

#[uniffi::export]
impl SessionHistory {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(super::SessionHistory::default()))
    }

    pub fn pj_uri(&self) -> Option<Arc<crate::PjUri>> {
        let guard = self.0.lock().unwrap();
        let uri = guard.pj_uri()?;
        Some(Arc::new(uri.into()))
    }

    pub fn payment_amount(&self) -> Option<Arc<crate::Amount>> {
        let guard = self.0.lock().unwrap();
        let amount = guard.payment_amount()?;
        Some(Arc::new(amount.into()))
    }

    pub fn payment_address(&self) -> Option<Arc<crate::Address>> {
        let guard = self.0.lock().unwrap();
        let address = guard.payment_address()?;
        Some(Arc::new(address.into()))
    }

    pub fn fallback_txid(&self) -> Option<String> {
        let guard = self.0.lock().unwrap();
        let txid = guard.fallback_txid()?;
        Some(txid.to_string())
    }

    pub fn proposal_txid(&self) -> Option<String> {
        let guard = self.0.lock().unwrap();
        let txid = guard.proposal_txid()?;
        Some(txid.to_string())
    }

    // TODO: move this out into its own exported fuinction
    // pub fn replay_receiver_event_log(
    //     &self,
    //     persister: Arc<dyn JsonReceiverPersistedSession>,
    // ) -> Result<UniReceiverState, ImplementationError> {
    //     let adapter = CallbackPersisterAdapter::new(persister);
    //     let res = self.0.lock().unwrap().replay_receiver_event_log(adapter.clone())?;

    //     Ok(res.into())
    // }
}

/// Adapter for the ReceiverPersister trait to use the save and load callbacks.
#[derive(Clone)]
struct CallbackPersisterAdapter {
    callback_persister: Arc<dyn JsonReceiverPersistedSession>,
}

impl CallbackPersisterAdapter {
    pub fn new(callback_persister: Arc<dyn JsonReceiverPersistedSession>) -> Self {
        Self { callback_persister }
    }
}

impl payjoin::persist::SessionPersister for CallbackPersisterAdapter {
    type SessionEvent = payjoin::receive::v2::ReceiverSessionEvent;
    type InternalStorageError = ForeignError;

    fn save_event(&self, event: &Self::SessionEvent) -> Result<(), Self::InternalStorageError> {
        let super_event: super::ReceiverSessionEvent = event.clone().into();
        let uni_event: UniReceiverSessionEvent = super_event.into();
        println!("Saving event: {}", uni_event.to_json());
        self.callback_persister.save(uni_event.to_json())
    }

    fn load(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Self::SessionEvent>>, Self::InternalStorageError> {
        let res = self.callback_persister.load()?;
        Ok(Box::new(
            // TODO: Remove unwrap
            res.into_iter()
                .map(|event| UniReceiverSessionEvent::from_json(event).unwrap().0.into()),
        ))
    }

    fn close(&self) -> Result<(), Self::InternalStorageError> {
        self.callback_persister.close()
    }
}
