import base64
from binascii import unhexlify
import os
import sys
import httpx

from payjoin import *
from typing import Optional
import payjoin.bitcoin as bitcoinffi

# The below sys path setting is required to use the 'payjoin' module in the 'src' directory
# This script is in the 'tests' directory and the 'payjoin' module is in the 'src' directory
sys.path.insert(
    0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "src"))
)

import hashlib
import unittest
from pprint import *
from bitcoin import SelectParams
from bitcoin.core.script import (
    CScript,
    OP_0,
    SignatureHash,
)
from bitcoin.wallet import *
from bitcoin.rpc import Proxy, hexlify_str, JSONRPCError

SelectParams("regtest")

def get_rpc_credentials_from_cookie(cookie_path):
    """Reads the RPC credentials from the cookie file"""
    with open(cookie_path, "r") as cookie_file:
        credentials = cookie_file.read().strip()
    return credentials.split(":")

# Function to create and load a wallet if it doesn't already exist
def create_and_load_wallet(rpc_connection, wallet_name):
    try:
        # Try to load the wallet using the _call method
        rpc_connection._call("loadwallet", wallet_name)
        print(f"Wallet '{wallet_name}' loaded successfully.")
    except JSONRPCError as e:
        # Check if the error code indicates the wallet does not exist
        if e.error["code"] == -18:  # Wallet not found error code
            # Create the wallet since it does not exist using the _call method
            rpc_connection._call("createwallet", wallet_name)
            print(f"Wallet '{wallet_name}' created and loaded successfully.")
        elif e.error["code"] == -35:  # Wallet already loaded
            print(f"Wallet '{wallet_name}' created and loaded successfully.")


# Set up RPC connections
rpc_user = os.environ.get("RPC_USER", "admin1")
rpc_password = os.environ.get("RPC_PASSWORD", "123")
rpc_host = os.environ.get("RPC_HOST", "localhost")
rpc_port = os.environ.get("RPC_PORT", "18443")
#ensure this is where your access cookie is located
# rpc_data_dir = os.environ.get("RPC_DATA_DIR", "~/.bitcoin/regtest") 
# cookie_path = os.path.expanduser(os.path.join(rpc_data_dir, ".cookie"))
# rpc_user, rpc_password = get_rpc_credentials_from_cookie(cookie_path)

# class InMemoryReceiverPersister(payjoin_ffi.ReceiverPersister):
#     def __init__(self):
#         super().__init__()
#         self.receivers = {}

#     def save(self, receiver: Receiver) -> ReceiverToken:
#         self.receivers[receiver.key().as_string()] = receiver.to_json()

#         return receiver.key()

#     def load(self, token: ReceiverToken) -> Receiver:
#         token = token.as_string()
#         if token not in self.receivers.keys():
#             raise ValueError(f"Token not found: {token}")
#         return Receiver.from_json(self.receivers[token])

# class InMemorySenderPersister(payjoin_ffi.SenderPersister):
#     def __init__(self):
#         super().__init__()
#         self.senders = {}

#     def save(self, sender: Sender) -> SenderToken:
#         self.senders[sender.key().as_string()] = sender.to_json()
#         return sender.key()

#     def load(self, token: SenderToken) -> Sender:
#         token = token.as_string()
#         if token not in self.senders.keys():
#             raise ValueError(f"Token not found: {token}")
#         return Sender.from_json(self.senders[token])
class ReceieverSessionEventLog(ReceiverPersistedSession):
    def __init__(self, id):
        self.id = id
        self.events = []
        self.closed = False

    # Str representation of the event
    def save(self, event: str):
        self.events.append(event)

    def load(self):
        return self.events

    def close(self):
        self.closed = True


class TestPayjoin(unittest.IsolatedAsyncioTestCase):
    ohttp_relay = None
    
    @classmethod
    def setUpClass(cls):
        # Initialize wallets once before all tests
        sender_wallet_name = "sender"
        sender_rpc_url = f"http://{rpc_user}:{rpc_password}@{rpc_host}:{rpc_port}/wallet/{sender_wallet_name}"
        cls.sender = Proxy(service_url=sender_rpc_url)
        create_and_load_wallet(cls.sender, sender_wallet_name)
        cls.sender.generatetoaddress(101, cls.sender.getnewaddress())

        receiver_wallet_name = "receiver"
        receiver_rpc_url = f"http://{rpc_user}:{rpc_password}@{rpc_host}:{rpc_port}/wallet/{receiver_wallet_name}"
        cls.receiver = Proxy(service_url=receiver_rpc_url)
        create_and_load_wallet(cls.receiver, receiver_wallet_name)
        cls.receiver.generatetoaddress(101, cls.receiver.getnewaddress())
 
    async def process_receiver_proposal(self, receiver: UniReceiverState) -> Optional[UniReceiverState]:
        if receiver.is_WITH_CONTEXT():
            res = await self.retrieve_receiver_proposal(receiver.inner)
            print(f"Retrieved receiver proposal: {res}")
            if not isinstance(res, UniReceiverState.UNCHECKED_PROPOSAL):
                return None
            return await self.process_receiver_proposal(res)
        
        if receiver.is_UNCHECKED_PROPOSAL():
            res = await self.process_unchecked_proposal(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_MAYBE_INPUTS_OWNED():
            res = await self.process_maybe_inputs_owned(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_MAYBE_INPUTS_SEEN():
            res = await self.process_maybe_inputs_seen(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_OUTPUTS_UNKNOWN():
            res = await self.process_outputs_unknown(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_WANTS_OUTPUTS():
            res = await self.process_wants_outputs(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_WANTS_INPUTS():
            res = await self.process_wants_inputs(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_PROVISIONAL_PROPOSAL():
            res = await self.process_provisional_proposal(receiver.inner)
            return await self.process_receiver_proposal(res)
        
        if receiver.is_PAYJOIN_PROPOSAL():
            return receiver
        
        raise Exception(f"Unknown receiver state: {receiver}")
            
            
    def create_receiver_context(self, receiver_address: bitcoinffi.Address, directory: Url, ohttp_keys: OhttpKeys, recv_persister: ReceieverSessionEventLog):
        receiver = UninitializedReceiver().create_session(address=receiver_address, directory=directory.as_string(), ohttp_keys=ohttp_keys, expire_after=None, persister=recv_persister)
        pj_uri = receiver.pj_uri()
        print(f"pj_uri: {pj_uri.as_string()}")
        
        return UniReceiverState.WITH_CONTEXT(receiver)
    
    async def retrieve_receiver_proposal(self, receiver: ReceiverWithContext):
        agent = httpx.AsyncClient()
        request: RequestResponse = receiver.extract_req(self.ohttp_relay.as_string())
        response = await agent.post(
            url=request.request.url.as_string(),
            headers={"Content-Type": request.request.content_type},
            content=request.request.body
        )
        res = receiver.process_res(response.content, request.client_response)
        if res == None:
            return None
        return UniReceiverState.UNCHECKED_PROPOSAL(res)
    
    async def process_unchecked_proposal(self, proposal: UncheckedProposal) -> UniReceiverState:
        receiver = proposal.check_broadcast_suitability(None, MempoolAcceptanceCallback(self.receiver))
        return UniReceiverState.MAYBE_INPUTS_OWNED(receiver)
    
    async def process_maybe_inputs_owned(self, proposal: MaybeInputsOwned) -> UniReceiverState:
        maybe_inputs_owned = proposal.check_inputs_not_owned(IsScriptOwnedCallback(self.receiver))
        return UniReceiverState.MAYBE_INPUTS_SEEN(maybe_inputs_owned)
    
    async def process_maybe_inputs_seen(self, proposal: MaybeInputsSeen) -> UniReceiverState:
        outputs_unknown = proposal.check_no_inputs_seen_before(IdentifyReceiverOutputsCallback(self.receiver))
        return UniReceiverState.OUTPUTS_UNKNOWN(outputs_unknown)
    
    async def process_outputs_unknown(self, proposal: OutputsUnknown) -> UniReceiverState:
        wants_outputs = proposal.identify_receiver_outputs(IsScriptOwnedCallback(self.receiver))
        return UniReceiverState.WANTS_OUTPUTS(wants_outputs)
    
    async def process_wants_outputs(self, proposal: WantsOutputs) -> UniReceiverState:
        wants_inputs = proposal.commit_outputs()
        return UniReceiverState.WANTS_INPUTS(wants_inputs)
    
    async def process_wants_inputs(self, proposal: WantsInputs) -> UniReceiverState:
        provisional_proposal = proposal.contribute_inputs(get_inputs(self.receiver)).commit_inputs()
        return UniReceiverState.PROVISIONAL_PROPOSAL(provisional_proposal)
    
    async def process_provisional_proposal(self, proposal: ProvisionalProposal) -> UniReceiverState:
        payjoin_proposal = proposal.finalize_proposal(ProcessPsbtCallback(self.receiver), 1, 10)
        return UniReceiverState.PAYJOIN_PROPOSAL(payjoin_proposal)
    
    async def test_integration_v2_to_v2(self):
        try:
            receiver_address = bitcoinffi.Address(str(self.receiver.getnewaddress()), bitcoinffi.Network.REGTEST)
            pre_payjoin_sender_balance = self.sender.getbalance()
            init_tracing()
            services = TestServices.initialize()

            # agent = services.http_agent()
            services.wait_for_services_ready()
            directory = services.directory_url()
            ohttp_keys = services.fetch_ohttp_keys()
            ohttp_relay = services.ohttp_relay_url()
            self.ohttp_relay = ohttp_relay

            # **********************
            # Inside the Receiver:
            recv_persister = ReceieverSessionEventLog(1)
            receiver = self.create_receiver_context(receiver_address, directory, ohttp_keys, recv_persister)
            response_body = await self.process_receiver_proposal(receiver)
            # No proposal yet since sender has not responded
            self.assertIsNone(response_body)
            pj_uri = receiver.inner.pj_uri() 
            # **********************
            # Inside the Sender:
            # Create a funded PSBT (not broadcasted) to address with amount given in the pj_uri
            agent = httpx.AsyncClient()
            outputs = {}
            outputs[pj_uri.address()] = 0.0001
            psbt = self.sender._call(
                "walletcreatefundedpsbt",
                [],
                outputs,
                0,
                {"lockUnspents": True, "fee_rate": 10, "subtract_fee_from_outputs": [0]},
                )["psbt"]
            psbt = self.sender._call("walletprocesspsbt", psbt, True, None, False)["psbt"]
            sender = SenderBuilder(psbt, pj_uri).build_recommended(1000).build()
            request: RequestV2PostContext = sender.extract_v2(ohttp_relay)
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            send_ctx: V2GetContext = request.context.process_response(response.content)
            # POST Original PSBT

            # **********************
            # Inside the Receiver:
            payjoin_proposal = await self.process_receiver_proposal(receiver)
            self.assertIsNotNone(payjoin_proposal)
            self.assertEqual(payjoin_proposal.is_PAYJOIN_PROPOSAL(), True)
            
            payjoin_proposal = payjoin_proposal.inner
            request: RequestResponse = payjoin_proposal.extract_req(ohttp_relay.as_string())
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            payjoin_proposal.process_res(response.content, request.client_response)
            
            events = recv_persister.load()
            events = recv_persister.events
            self.assertEqual(len(events), 9)
            history = SessionHistory()
            historical_pj_uri = history.pj_uri()
            self.assertEqual(historical_pj_uri, None)
            
            recv_state = history.replay_receiver_event_log(recv_persister)
            
            self.assertEqual(recv_state.is_PAYJOIN_PROPOSAL(), True)
            self.assertEqual(recv_persister.closed, True)
            
            historical_pj_uri = history.pj_uri()
            self.assertEqual(historical_pj_uri.as_string(), pj_uri.as_string())
            
            # address = history.receiving_address()
            # print(f"address: {address}")
            # Can't compare b/c to display implenent on address
            # self.assertEqual(address.to_qr_uri(), pj_uri.address())
            return;
            
            # **********************
            # Inside the Sender:
            # Sender checks, signs, finalizes, extracts, and broadcasts
            # Replay post fallback to get the response
            request: RequestOhttpContext = send_ctx.extract_req(ohttp_relay.as_string())
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            try:
                checked_payjoin_proposal_psbt: Optional[str] = send_ctx.process_response(response.content, request.ohttp_ctx)
            except Exception as e:
                print(response_error_to_json(e))
                raise
            self.assertIsNotNone(checked_payjoin_proposal_psbt)
            payjoin_psbt = self.sender._call("walletprocesspsbt", checked_payjoin_proposal_psbt, True, None, False)["psbt"]
            final_psbt = self.sender._call("finalizepsbt", payjoin_psbt, False)["psbt"]
            # print(f"Final psbt: {final_psbt}")
            payjoin_tx = bitcoinffi.Psbt.deserialize_base64(final_psbt).extract_tx()
            self.sender.sendrawtransaction(payjoin_tx)
            # print(f"Tx sent: {payjoin_tx.compute_txid()}")

            # Check resulting transaction and balances
            # network_fees = bitcoinffi.predicted_tx_weight(payjoin_tx) * 1000;
            # Sender sent the entire value of their utxo to receiver (minus fees)
            self.assertEqual(len(payjoin_tx.input()), 2);
            self.assertEqual(len(payjoin_tx.output()), 2);
            # self.assertEqual(self.receiver.getbalance(), bitcoinffi.Amount.from_btc(100.0) - network_fees)
            # self.assertEqual(self.sender.getbalance(), pre_payjoin_sender_balance - 10000)
            return payjoin_tx
        except Exception as e:
            print("Caught:", e)
            raise

def get_inputs(rpc_connection: Proxy) -> list[InputPair]:
    utxos = rpc_connection._call("listunspent")
    inputs = []
    for utxo in utxos[:1]:
        txin = bitcoinffi.TxIn(
            previous_output=bitcoinffi.OutPoint(txid=utxo["txid"], vout=utxo["vout"]),
            script_sig=bitcoinffi.Script(bytes()),
            sequence=0,
            witness=[]
        )
        raw_tx = rpc_connection._call("getrawtransaction", utxo["txid"], True)
        prev_out = raw_tx["vout"][utxo["vout"]]
        prev_spk = bitcoinffi.Script(bytes.fromhex(prev_out["scriptPubKey"]["hex"]))
        prev_amount = bitcoinffi.Amount.from_btc(prev_out["value"])
        tx_out = bitcoinffi.TxOut(value=prev_amount, script_pubkey=prev_spk)
        psbt_in = PsbtInput(witness_utxo=tx_out, redeem_script=None, witness_script=None)
        inputs.append(InputPair(txin=txin, psbtin=psbt_in))

    return inputs

class MempoolAcceptanceCallback(CanBroadcast):
    def __init__(self, connection: Proxy):
        self.connection = connection

    def callback(self, tx):
          try:
                res = self.connection._call("testmempoolaccept", [bytes(tx).hex()])[0][
                    "allowed"
                ]
                return res
          except Exception as e:
            print(f"An error occurred: {e}")
            return None      

class IsScriptOwnedCallback(IsScriptOwned):
    def __init__(self, connection: Proxy):
        self.connection = connection

    def callback(self, script):
        try:
            address = bitcoinffi.Address.from_script(bitcoinffi.Script(script), bitcoinffi.Network.REGTEST)
            return self.connection._call("getaddressinfo", str(address))["ismine"]
        except Exception as e:
            print(f"An error occurred: {e}")
            return None

class IdentifyReceiverOutputsCallback(IsOutputKnown):
    def __init__(self, connection: Proxy):
        self.connection = connection

    def callback(self, outpoint):
        return False

class ProcessPsbtCallback(ProcessPsbt):
    def __init__(self, connection: Proxy):
        self.connection = connection

    def callback(self, psbt: str):
        res = self.connection._call("walletprocesspsbt", psbt)
        return res['psbt']

if __name__ == "__main__":
    unittest.main()
