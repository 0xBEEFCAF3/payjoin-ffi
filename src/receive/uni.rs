use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use super::{InputPair, ReceiverState};
use crate::bitcoin_ffi::{Address, OutPoint, Script, TxOut};
use crate::error::ForeignError;
pub use crate::receive::{
    Error, ImplementationError, InputContributionError, JsonReply, OutputSubstitutionError,
    ReplyableError, SelectionError, SerdeJsonError, SessionError,
};
use crate::uri::error::IntoUrlError;
use crate::{ClientResponse, OhttpKeys, OutputSubstitution, Request};

macro_rules! impl_json_methods {
    ($type:ty) => {
        #[uniffi::export]
        impl $type {
            pub fn to_json(&self) -> String {
                serde_json::to_string(&self.0).unwrap()
            }

            #[uniffi::constructor]
            pub fn from_json(json: String) -> Result<Self, UniReceiverError> {
                let inner = serde_json::from_str(&json).unwrap();
                Ok(Self(inner))
            }
        }
    };
}

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

macro_rules! impl_from_payjoin_methods {
    ($uni_type:ty, $payjoin_type:ty) => {
        impl From<$uni_type> for $payjoin_type {
            fn from(value: $uni_type) -> Self {
                value.0.into()
            }
        }

        impl From<$payjoin_type> for $uni_type {
            fn from(value: $payjoin_type) -> Self {
                Self(value.into())
            }
        }
    };
}
#[derive(uniffi::Object)]
pub struct UninitializedReceiver(pub(crate) super::UninitializedReceiver);

impl From<super::UninitializedReceiver> for UninitializedReceiver {
    fn from(value: super::UninitializedReceiver) -> Self {
        Self(value)
    }
}

#[uniffi::export]
impl UninitializedReceiver {
    #[uniffi::constructor]
    pub fn new() -> Self {
        // TODO: should just replace with default impls
        Self(super::UninitializedReceiver(payjoin::receive::v2::UninitializedReceiver {}))
    }

    pub fn create_session(
        &self,
        address: Arc<Address>,
        directory: String,
        ohttp_keys: Arc<OhttpKeys>,
        expire_after: Option<u64>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<ReceiverWithContext, IntoUrlError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        let receiver = super::UninitializedReceiver::create_session(
            (*address).clone(),
            directory,
            (*ohttp_keys).clone(),
            expire_after,
            adapter,
        )?;
        Ok(receiver.into())
    }
}

#[derive(uniffi::Object, Clone)]
pub struct ReceiverWithContext(super::ReceiverWithContext);

impl_from_super_methods!(ReceiverWithContext, super::ReceiverWithContext);
impl_from_payjoin_methods!(ReceiverWithContext, payjoin::receive::v2::ReceiverWithContext);

#[uniffi::export]
impl ReceiverWithContext {
    pub fn extract_req(&self, ohttp_relay: String) -> Result<RequestResponse, Error> {
        self.0
            .extract_req(ohttp_relay)
            .map(|(request, ctx)| RequestResponse { request, client_response: Arc::new(ctx) })
    }

    pub fn process_res(
        &self,
        body: &[u8],
        context: Arc<ClientResponse>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Option<Arc<UncheckedProposal>>, Error> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0.process_res(body, &context, adapter).map(|e| e.map(|x| Arc::new(x.into())))
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

/// The sender’s original PSBT and optional parameters
///
/// This type is used to proces the request. It is returned by UncheckedProposal::from_request().
///
/// If you are implementing an interactive payment processor, you should get extract the original transaction with get_transaction_to_schedule_broadcast() and schedule, followed by checking that the transaction can be broadcast with check_can_broadcast. Otherwise it is safe to call assume_interactive_receive to proceed with validation.
#[derive(Clone, uniffi::Object)]
pub struct UncheckedProposal(super::UncheckedProposal);

impl_from_super_methods!(UncheckedProposal, super::UncheckedProposal);
impl_from_payjoin_methods!(UncheckedProposal, payjoin::receive::v2::UncheckedProposal);

#[uniffi::export]
impl UncheckedProposal {
    /// The Sender’s Original PSBT
    pub fn extract_tx_to_schedule_broadcast(&self) -> Vec<u8> {
        self.0.extract_tx_to_schedule_broadcast()
    }

    /// Call after checking that the Original PSBT can be broadcast.
    ///
    /// Receiver MUST check that the Original PSBT from the sender can be broadcast, i.e. testmempoolaccept bitcoind rpc returns { “allowed”: true,.. } for get_transaction_to_check_broadcast() before calling this method.
    ///
    /// Do this check if you generate bitcoin uri to receive Payjoin on sender request without manual human approval, like a payment processor. Such so called “non-interactive” receivers are otherwise vulnerable to probing attacks. If a sender can make requests at will, they can learn which bitcoin the receiver owns at no cost. Broadcasting the Original PSBT after some time in the failure case makes incurs sender cost and prevents probing.
    ///
    /// Call this after checking downstream.
    pub fn check_broadcast_suitability(
        &self,
        min_fee_rate: Option<u64>,
        can_broadcast: Arc<dyn CanBroadcast>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Arc<MaybeInputsOwned>, ReplyableError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0
            .clone()
            .check_broadcast_suitability(
                min_fee_rate,
                |transaction| {
                    can_broadcast
                        .callback(transaction.to_vec())
                        .map_err(|e| ImplementationError::from(e.to_string()))
                },
                adapter,
            )
            .map(|e| Arc::new(e.into()))
    }

    /// Call this method if the only way to initiate a Payjoin with this receiver
    /// requires manual intervention, as in most consumer wallets.
    ///
    /// So-called "non-interactive" receivers, like payment processors, that allow arbitrary requests are otherwise vulnerable to probing attacks.
    /// Those receivers call `extract_tx_to_check_broadcast()` and `attest_tested_and_scheduled_broadcast()` after making those checks downstream.
    pub fn assume_interactive_receiver(&self) -> Arc<MaybeInputsOwned> {
        Arc::new(self.0.assume_interactive_receiver().into())
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
impl_from_payjoin_methods!(MaybeInputsOwned, payjoin::receive::v2::MaybeInputsOwned);

#[uniffi::export(with_foreign)]
pub trait IsScriptOwned: Send + Sync {
    fn callback(&self, script: Vec<u8>) -> Result<bool, ForeignError>;
}

#[uniffi::export]
impl MaybeInputsOwned {
    ///Check that the Original PSBT has no receiver-owned inputs. Return original-psbt-rejected error or otherwise refuse to sign undesirable inputs.
    /// An attacker could try to spend receiver's own inputs. This check prevents that.
    pub fn check_inputs_not_owned(
        &self,
        is_owned: Arc<dyn IsScriptOwned>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Arc<MaybeInputsSeen>, ReplyableError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0
            .check_inputs_not_owned(
                |input| {
                    is_owned
                        .callback(input.to_vec())
                        .map_err(|e| ImplementationError::from(e.to_string()))
                },
                adapter,
            )
            .map(|t| Arc::new(t.into()))
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
impl_from_payjoin_methods!(MaybeInputsSeen, payjoin::receive::v2::MaybeInputsSeen);

#[uniffi::export]
impl MaybeInputsSeen {
    /// Make sure that the original transaction inputs have never been seen before. This prevents probing attacks. This prevents reentrant Payjoin, where a sender proposes a Payjoin PSBT as a new Original PSBT for a new Payjoin.
    pub fn check_no_inputs_seen_before(
        &self,
        is_known: Arc<dyn IsOutputKnown>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Arc<OutputsUnknown>, ReplyableError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0
            .clone()
            .check_no_inputs_seen_before(
                |outpoint| {
                    is_known
                        .callback(outpoint.clone())
                        .map_err(|e| ImplementationError::from(e.to_string()))
                },
                adapter,
            )
            .map(|t| Arc::new(t.into()))
    }
}

/// The receiver has not yet identified which outputs belong to the receiver.
///
/// Only accept PSBTs that send us money. Identify those outputs with identify_receiver_outputs() to proceed
#[derive(Clone, uniffi::Object)]
pub struct OutputsUnknown(super::OutputsUnknown);

impl_from_super_methods!(OutputsUnknown, super::OutputsUnknown);
impl_from_payjoin_methods!(OutputsUnknown, payjoin::receive::v2::OutputsUnknown);

#[uniffi::export]
impl OutputsUnknown {
    /// Find which outputs belong to the receiver
    pub fn identify_receiver_outputs(
        &self,
        is_receiver_output: Arc<dyn IsScriptOwned>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Arc<WantsOutputs>, ReplyableError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0
            .clone()
            .identify_receiver_outputs(
                |output_script| {
                    is_receiver_output
                        .callback(output_script.to_vec())
                        .map_err(|e| ImplementationError::from(e.to_string()))
                },
                adapter,
            )
            .map(|t| Arc::new(t.into()))
    }
}

#[derive(uniffi::Object)]
pub struct WantsOutputs(super::WantsOutputs);

impl_from_super_methods!(WantsOutputs, super::WantsOutputs);
impl_from_payjoin_methods!(WantsOutputs, payjoin::receive::v2::WantsOutputs);

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

    pub fn commit_outputs(&self, persister: Arc<dyn ReceiverPersistedSession>) -> Arc<WantsInputs> {
        let adapter = CallbackPersisterAdapter::new(persister);
        Arc::new(self.0.commit_outputs(adapter).into())
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
impl_from_payjoin_methods!(WantsInputs, payjoin::receive::v2::WantsInputs);

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

    pub fn commit_inputs(
        &self,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Arc<ProvisionalProposal> {
        let adapter = CallbackPersisterAdapter::new(persister);
        Arc::new(self.0.commit_inputs(adapter).into())
    }
}

#[derive(uniffi::Object)]
pub struct ProvisionalProposal(super::ProvisionalProposal);

impl_from_super_methods!(ProvisionalProposal, super::ProvisionalProposal);
impl_from_payjoin_methods!(ProvisionalProposal, payjoin::receive::v2::ProvisionalProposal);

/// A mutable checked proposal that the receiver may contribute inputs to to make a payjoin.
#[uniffi::export]
impl ProvisionalProposal {
    pub fn finalize_proposal(
        &self,
        process_psbt: Arc<dyn ProcessPsbt>,
        min_feerate_sat_per_vb: Option<u64>,
        max_effective_fee_rate_sat_per_vb: Option<u64>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<Arc<PayjoinProposal>, ReplyableError> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0
            .finalize_proposal(
                |psbt| {
                    process_psbt
                        .callback(psbt.to_string())
                        .map_err(|e| ImplementationError::from(e.to_string()))
                },
                min_feerate_sat_per_vb,
                max_effective_fee_rate_sat_per_vb,
                adapter,
            )
            .map(|e| Arc::new(e.into()))
    }
}

#[uniffi::export(with_foreign)]
pub trait ProcessPsbt: Send + Sync {
    fn callback(&self, psbt: String) -> Result<String, ForeignError>;
}

#[derive(Clone, uniffi::Object)]
pub struct PayjoinProposal(super::PayjoinProposal);

impl_from_super_methods!(PayjoinProposal, super::PayjoinProposal);
impl_from_payjoin_methods!(PayjoinProposal, payjoin::receive::v2::PayjoinProposal);

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
    pub fn process_res(
        &self,
        body: &[u8],
        ctx: Arc<ClientResponse>,
        persister: Arc<dyn ReceiverPersistedSession>,
    ) -> Result<(), Error> {
        let adapter = CallbackPersisterAdapter::new(persister);
        self.0.process_res(body, ctx.as_ref(), adapter)
    }
}

#[uniffi::export(with_foreign)]
pub trait ReceiverPersistedSession: Send + Sync {
    fn save(&self, event: UniReceiverSessionEvent) -> Result<(), ForeignError>;
    fn load(&self) -> Result<Vec<UniReceiverSessionEvent>, ForeignError>;
    fn close(&self) -> Result<(), ForeignError>;
}

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniReceiverSessionContext(payjoin::receive::v2::SessionContext);

impl_json_methods!(UniReceiverSessionContext);
impl_from_super_methods!(UniReceiverSessionContext, payjoin::receive::v2::SessionContext);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniUncheckedProposal(payjoin::receive::v1::UncheckedProposal);

impl_json_methods!(UniUncheckedProposal);
impl_from_super_methods!(UniUncheckedProposal, payjoin::receive::v1::UncheckedProposal);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniMaybeInputsOwned(payjoin::receive::v1::MaybeInputsOwned);

impl_json_methods!(UniMaybeInputsOwned);
impl_from_super_methods!(UniMaybeInputsOwned, payjoin::receive::v1::MaybeInputsOwned);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniMaybeInputsSeen(payjoin::receive::v1::MaybeInputsSeen);

impl_json_methods!(UniMaybeInputsSeen);
impl_from_super_methods!(UniMaybeInputsSeen, payjoin::receive::v1::MaybeInputsSeen);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniOutputsUnknown(payjoin::receive::v1::OutputsUnknown);

impl_json_methods!(UniOutputsUnknown);
impl_from_super_methods!(UniOutputsUnknown, payjoin::receive::v1::OutputsUnknown);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniWantsOutputs(payjoin::receive::v1::WantsOutputs);

impl_json_methods!(UniWantsOutputs);
impl_from_super_methods!(UniWantsOutputs, payjoin::receive::v1::WantsOutputs);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniWantsInputs(payjoin::receive::v1::WantsInputs);

impl_json_methods!(UniWantsInputs);
impl_from_super_methods!(UniWantsInputs, payjoin::receive::v1::WantsInputs);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniProvisionalProposal(payjoin::receive::v1::ProvisionalProposal);

impl_json_methods!(UniProvisionalProposal);
impl_from_super_methods!(UniProvisionalProposal, payjoin::receive::v1::ProvisionalProposal);

#[derive(Clone, uniffi::Object, serde::Serialize, serde::Deserialize, Debug)]
pub struct UniPayjoinProposal(payjoin::receive::v1::PayjoinProposal);

impl_json_methods!(UniPayjoinProposal);
impl_from_super_methods!(UniPayjoinProposal, payjoin::receive::v1::PayjoinProposal);

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum UniReceiverError {
    #[error("Some error")]
    SomeError(String),
}

#[derive(Clone, uniffi::Enum, serde::Serialize, serde::Deserialize)]
pub enum UniReceiverSessionEvent {
    Created { inner: Arc<UniReceiverSessionContext> },
    UncheckedProposal { inner: Arc<UniUncheckedProposal> },
    MaybeInputsOwned { inner: Arc<UniMaybeInputsOwned> },
    MaybeInputsSeen { inner: Arc<UniMaybeInputsSeen> },
    OutputsUnknown { inner: Arc<UniOutputsUnknown> },
    WantsOutputs { inner: Arc<UniWantsOutputs> },
    WantsInputs { inner: Arc<UniWantsInputs> },
    ProvisionalProposal { inner: Arc<UniProvisionalProposal> },
    PayjoinProposal { inner: Arc<UniPayjoinProposal> },
    FallbackBroadcasted { txid: String },
    SessionInvalid { reason: String },
}

impl From<payjoin::receive::v2::ReceiverSessionEvent> for UniReceiverSessionEvent {
    fn from(value: payjoin::receive::v2::ReceiverSessionEvent) -> Self {
        match value {
            payjoin::receive::v2::ReceiverSessionEvent::Created(context) => {
                Self::Created { inner: Arc::new(context.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::UncheckedProposal(proposal) => {
                Self::UncheckedProposal { inner: Arc::new(proposal.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsOwned(inputs) => {
                Self::MaybeInputsOwned { inner: Arc::new(inputs.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsSeen(inputs) => {
                Self::MaybeInputsSeen { inner: Arc::new(inputs.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::OutputsUnknown(unknown) => {
                Self::OutputsUnknown { inner: Arc::new(unknown.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::WantsOutputs(outputs) => {
                Self::WantsOutputs { inner: Arc::new(outputs.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::WantsInputs(inputs) => {
                Self::WantsInputs { inner: Arc::new(inputs.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::ProvisionalProposal(proposal) => {
                Self::ProvisionalProposal { inner: Arc::new(proposal.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::PayjoinProposal(proposal) => {
                Self::PayjoinProposal { inner: Arc::new(proposal.into()) }
            }
            payjoin::receive::v2::ReceiverSessionEvent::FallbackBroadcasted(txid) => {
                Self::FallbackBroadcasted { txid: txid.to_string() }
            }
            payjoin::receive::v2::ReceiverSessionEvent::SessionInvalid(reason) => {
                Self::SessionInvalid { reason }
            }
        }
    }
}

impl From<UniReceiverSessionEvent> for payjoin::receive::v2::ReceiverSessionEvent {
    fn from(value: UniReceiverSessionEvent) -> Self {
        match value {
            UniReceiverSessionEvent::Created { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::Created(inner.0.clone())
            }
            UniReceiverSessionEvent::UncheckedProposal { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::UncheckedProposal(inner.0.clone())
            }
            UniReceiverSessionEvent::MaybeInputsOwned { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsOwned(inner.0.clone())
            }
            UniReceiverSessionEvent::MaybeInputsSeen { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsSeen(inner.0.clone())
            }
            UniReceiverSessionEvent::OutputsUnknown { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::OutputsUnknown(inner.0.clone())
            }
            UniReceiverSessionEvent::WantsOutputs { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::WantsOutputs(inner.0.clone())
            }
            UniReceiverSessionEvent::WantsInputs { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::WantsInputs(inner.0.clone())
            }
            UniReceiverSessionEvent::ProvisionalProposal { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::ProvisionalProposal(inner.0.clone())
            }
            UniReceiverSessionEvent::PayjoinProposal { inner } => {
                payjoin::receive::v2::ReceiverSessionEvent::PayjoinProposal(inner.0.clone())
            }
            UniReceiverSessionEvent::FallbackBroadcasted { txid } => {
                payjoin::receive::v2::ReceiverSessionEvent::FallbackBroadcasted(
                    payjoin::bitcoin::Txid::from_str(&txid).unwrap(),
                )
            }
            UniReceiverSessionEvent::SessionInvalid { reason } => {
                payjoin::receive::v2::ReceiverSessionEvent::SessionInvalid(reason.clone())
            }
        }
    }
}

impl payjoin::persist::Event for UniReceiverSessionEvent {
    fn session_invalid(error: &impl payjoin::persist::PersistableError) -> Self {
        Self::SessionInvalid { reason: error.to_string() }
    }
}

#[uniffi::export]
fn to_json(event: UniReceiverSessionEvent) -> String {
    event.to_json()
}

#[uniffi::export]
fn from_json(json: String) -> Result<UniReceiverSessionEvent, UniReceiverError> {
    UniReceiverSessionEvent::from_json(json)
}

impl UniReceiverSessionEvent {
    pub fn to_json(&self) -> String {
        let event = match self {
            UniReceiverSessionEvent::Created { inner } => {
                let inner = payjoin::receive::v2::SessionContext::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::Created(inner)
            }
            UniReceiverSessionEvent::UncheckedProposal { inner } => {
                let inner = payjoin::receive::v1::UncheckedProposal::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::UncheckedProposal(inner)
            }
            UniReceiverSessionEvent::MaybeInputsOwned { inner } => {
                let inner = payjoin::receive::v1::MaybeInputsOwned::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsOwned(inner)
            }
            UniReceiverSessionEvent::MaybeInputsSeen { inner } => {
                let inner = payjoin::receive::v1::MaybeInputsSeen::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::MaybeInputsSeen(inner)
            }
            UniReceiverSessionEvent::OutputsUnknown { inner } => {
                let inner = payjoin::receive::v1::OutputsUnknown::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::OutputsUnknown(inner)
            }
            UniReceiverSessionEvent::WantsOutputs { inner } => {
                let inner = payjoin::receive::v1::WantsOutputs::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::WantsOutputs(inner)
            }
            UniReceiverSessionEvent::WantsInputs { inner } => {
                let inner = payjoin::receive::v1::WantsInputs::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::WantsInputs(inner)
            }
            UniReceiverSessionEvent::ProvisionalProposal { inner } => {
                let inner = payjoin::receive::v1::ProvisionalProposal::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::ProvisionalProposal(inner)
            }
            UniReceiverSessionEvent::PayjoinProposal { inner } => {
                let inner = payjoin::receive::v1::PayjoinProposal::from(inner.0.clone());
                payjoin::receive::v2::ReceiverSessionEvent::PayjoinProposal(inner)
            }
            UniReceiverSessionEvent::FallbackBroadcasted { txid } => {
                // TODO: intenral type should use bitcoin::Txid
                payjoin::receive::v2::ReceiverSessionEvent::FallbackBroadcasted(
                    payjoin::bitcoin::Txid::from_str(&txid).unwrap(),
                )
            }
            UniReceiverSessionEvent::SessionInvalid { reason } => {
                payjoin::receive::v2::ReceiverSessionEvent::SessionInvalid(reason.clone())
            }
        };
        serde_json::to_string(&event).unwrap()
    }

    pub fn from_json(json: String) -> Result<Self, UniReceiverError> {
        let event: payjoin::receive::v2::ReceiverSessionEvent =
            serde_json::from_str(&json).unwrap();
        let event = super::ReceiverSessionEvent::from(event);
        Ok(event.into())
    }
}

impl From<UniReceiverSessionEvent> for super::ReceiverSessionEvent {
    fn from(value: UniReceiverSessionEvent) -> Self {
        match value {
            UniReceiverSessionEvent::Created { inner } => {
                super::ReceiverSessionEvent::Created((*inner).clone().into())
            }
            UniReceiverSessionEvent::UncheckedProposal { inner } => {
                super::ReceiverSessionEvent::UncheckedProposal((*inner).clone().into())
            }
            UniReceiverSessionEvent::MaybeInputsOwned { inner } => {
                super::ReceiverSessionEvent::MaybeInputsOwned((*inner).clone().into())
            }
            UniReceiverSessionEvent::MaybeInputsSeen { inner } => {
                super::ReceiverSessionEvent::MaybeInputsSeen((*inner).clone().into())
            }
            UniReceiverSessionEvent::OutputsUnknown { inner } => {
                super::ReceiverSessionEvent::OutputsUnknown((*inner).clone().into())
            }
            UniReceiverSessionEvent::WantsOutputs { inner } => {
                super::ReceiverSessionEvent::WantsOutputs((*inner).clone().into())
            }
            UniReceiverSessionEvent::WantsInputs { inner } => {
                super::ReceiverSessionEvent::WantsInputs((*inner).clone().into())
            }
            UniReceiverSessionEvent::ProvisionalProposal { inner } => {
                super::ReceiverSessionEvent::ProvisionalProposal((*inner).clone().into())
            }
            UniReceiverSessionEvent::PayjoinProposal { inner } => {
                super::ReceiverSessionEvent::PayjoinProposal((*inner).clone().into())
            }
            UniReceiverSessionEvent::FallbackBroadcasted { txid } => {
                super::ReceiverSessionEvent::FallbackBroadcasted(
                    payjoin::bitcoin::Txid::from_str(&txid).unwrap(),
                )
            }
            UniReceiverSessionEvent::SessionInvalid { reason } => {
                super::ReceiverSessionEvent::SessionInvalid(reason.clone())
            }
        }
    }
}

impl From<super::ReceiverSessionEvent> for UniReceiverSessionEvent {
    fn from(value: super::ReceiverSessionEvent) -> Self {
        match value {
            super::ReceiverSessionEvent::Created(context) => {
                UniReceiverSessionEvent::Created { inner: Arc::new(context.into()) }
            }
            super::ReceiverSessionEvent::UncheckedProposal(proposal) => {
                UniReceiverSessionEvent::UncheckedProposal { inner: Arc::new(proposal.into()) }
            }
            super::ReceiverSessionEvent::MaybeInputsOwned(inputs) => {
                UniReceiverSessionEvent::MaybeInputsOwned { inner: Arc::new(inputs.into()) }
            }
            super::ReceiverSessionEvent::MaybeInputsSeen(inputs) => {
                UniReceiverSessionEvent::MaybeInputsSeen { inner: Arc::new(inputs.into()) }
            }
            super::ReceiverSessionEvent::OutputsUnknown(unknown) => {
                UniReceiverSessionEvent::OutputsUnknown { inner: Arc::new(unknown.into()) }
            }
            super::ReceiverSessionEvent::WantsOutputs(outputs) => {
                UniReceiverSessionEvent::WantsOutputs { inner: Arc::new(outputs.into()) }
            }
            super::ReceiverSessionEvent::WantsInputs(inputs) => {
                UniReceiverSessionEvent::WantsInputs { inner: Arc::new(inputs.into()) }
            }
            super::ReceiverSessionEvent::ProvisionalProposal(proposal) => {
                UniReceiverSessionEvent::ProvisionalProposal { inner: Arc::new(proposal.into()) }
            }
            super::ReceiverSessionEvent::PayjoinProposal(proposal) => {
                UniReceiverSessionEvent::PayjoinProposal { inner: Arc::new(proposal.into()) }
            }
            super::ReceiverSessionEvent::FallbackBroadcasted(txid) => {
                UniReceiverSessionEvent::FallbackBroadcasted { txid: txid.to_string() }
            }
            super::ReceiverSessionEvent::SessionInvalid(reason) => {
                UniReceiverSessionEvent::SessionInvalid { reason }
            }
        }
    }
}

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

impl From<super::ReceiverState> for UniReceiverState {
    fn from(value: super::ReceiverState) -> Self {
        match value {
            super::ReceiverState::Uninitialized(inner) => UniReceiverState::Uninitialized,
            super::ReceiverState::WithContext(inner) => {
                UniReceiverState::WithContext { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::UncheckedProposal(inner) => {
                UniReceiverState::UncheckedProposal { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::MaybeInputsOwned(inner) => {
                UniReceiverState::MaybeInputsOwned { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::MaybeInputsSeen(inner) => {
                UniReceiverState::MaybeInputsSeen { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::OutputsUnknown(inner) => {
                UniReceiverState::OutputsUnknown { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::WantsOutputs(inner) => {
                UniReceiverState::WantsOutputs { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::WantsInputs(inner) => {
                UniReceiverState::WantsInputs { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::ProvisionalProposal(inner) => {
                UniReceiverState::ProvisionalProposal { inner: Arc::new(inner.into()) }
            }
            super::ReceiverState::PayjoinProposal(inner) => {
                UniReceiverState::PayjoinProposal { inner: Arc::new(inner.into()) }
            }
            _ => todo!("need to impl uni receiver state from v2 receiver state"),
        }
    }
}

#[uniffi::export]
pub fn replay_receiver_event_log(
    persister: Arc<dyn ReceiverPersistedSession>,
) -> Result<UniReceiverState, ImplementationError> {
    let adapter = CallbackPersisterAdapter::new(persister);
    let res = super::replay_receiver_event_log(adapter).unwrap();
    Ok(res.into())
}

/// Adapter for the ReceiverPersister trait to use the save and load callbacks.
#[derive(Clone)]
struct CallbackPersisterAdapter {
    callback_persister: Arc<dyn ReceiverPersistedSession>,
}

impl CallbackPersisterAdapter {
    pub fn new(callback_persister: Arc<dyn ReceiverPersistedSession>) -> Self {
        Self { callback_persister }
    }
}

impl payjoin::persist::PersistedSession for CallbackPersisterAdapter {
    type SessionEvent = UniReceiverSessionEvent;
    type Error = ForeignError;

    fn save(&self, event: Self::SessionEvent) -> Result<(), Self::Error> {
        self.callback_persister.save(event.into())
    }

    fn load(&self) -> Result<Box<dyn Iterator<Item = Self::SessionEvent>>, Self::Error> {
        println!("Loading events...");
        let res = self.callback_persister.load()?;
        println!("Loaded {:?} events", res.len());
        Ok(Box::new(res.into_iter().map(|event| event)))
    }

    fn close(&self) -> Result<(), Self::Error> {
        self.callback_persister.close()
    }
}
