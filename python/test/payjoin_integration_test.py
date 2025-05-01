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
rpc_data_dir = os.environ.get("RPC_DATA_DIR", "~/.bitcoin/regtest") 
cookie_path = os.path.expanduser(os.path.join(rpc_data_dir, ".cookie"))
rpc_user, rpc_password = get_rpc_credentials_from_cookie(cookie_path)

class InMemoryReceiverPersister(ReceiverPersister):
    def __init__(self):
        super().__init__()
        self.receivers = {}

    def save(self, receiver: Receiver) -> ReceiverToken:
        self.receivers[str(receiver.key())] = receiver.to_json()

        return receiver.key()

    def load(self, token: ReceiverToken) -> Receiver:
        token = str(token)
        if token not in self.receivers.keys():
            raise ValueError(f"Token not found: {token}")
        return Receiver.from_json(self.receivers[token])

class InMemorySenderPersister(SenderPersister):
    def __init__(self):
        super().__init__()
        self.senders = {}

    def save(self, sender: Sender) -> SenderToken:
        self.senders[str(sender.key())] = sender.to_json()
        return sender.key()

    def load(self, token: SenderToken) -> Sender:
        token = str(token)
        if token not in self.senders.keys():
            raise ValueError(f"Token not found: {token}")
        return Sender.from_json(self.senders[token])

class TestPayjoin(unittest.IsolatedAsyncioTestCase):
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

            # **********************
            # Inside the Receiver:
            expiry: Optional[int] = None
            new_receiver = NewReceiver(receiver_address, directory.as_string(), ohttp_keys, expiry)
            persister = InMemoryReceiverPersister()
            token = new_receiver.persist(persister)
            session: Receiver = Receiver.load(token, persister)
            print(f"session: {session.to_json()}")
            # Poll receive request
            ohttp_relay = services.ohttp_relay_url()
            request: RequestResponse = session.extract_req(ohttp_relay.as_string())
            agent = httpx.AsyncClient()
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            response_body = session.process_res(response.content, request.client_response)
            # No proposal yet since sender has not responded
            self.assertIsNone(response_body)
            
            # **********************
            # Inside the Sender:
            # Create a funded PSBT (not broadcasted) to address with amount given in the pj_uri
            pj_uri = session.pj_uri()
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
            new_sender = SenderBuilder(psbt, pj_uri).build_recommended(1000)
            persister = InMemorySenderPersister()
            token = new_sender.persist(persister)
            req_ctx: Sender = Sender.load(token, persister)
            request: RequestV2PostContext = req_ctx.extract_v2(ohttp_relay)
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            send_ctx: V2GetContext = request.context.process_response(response.content)
            # POST Original PSBT

            # **********************
            # Inside the Receiver:

            # GET fallback psbt
            request: RequestResponse = session.extract_req(ohttp_relay.as_string())
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            # POST payjoin
            proposal = session.process_res(response.content, request.client_response)
            maybe_inputs_owned = proposal.check_broadcast_suitability(None, MempoolAcceptanceCallback(self.receiver))
            maybe_inputs_seen = maybe_inputs_owned.check_inputs_not_owned(IsScriptOwnedCallback(self.receiver))
            outputs_unknown = maybe_inputs_seen.check_no_inputs_seen_before(IdentifyReceiverOutputsCallback(self.receiver))
            wants_outputs = outputs_unknown.identify_receiver_outputs(IsScriptOwnedCallback(self.receiver))
            wants_inputs = wants_outputs.commit_outputs()
            provisional_proposal = wants_inputs.contribute_inputs(get_inputs(self.receiver)).commit_inputs()
            payjoin_proposal = provisional_proposal.finalize_proposal(ProcessPsbtCallback(self.receiver), 1, 10)
            # print(f"Proposal psbt: {payjoin_proposal.psbt()}")
            request: RequestResponse = payjoin_proposal.extract_req(ohttp_relay.as_string())
            response = await agent.post(
                url=request.request.url.as_string(),
                headers={"Content-Type": request.request.content_type},
                content=request.request.body
            )
            payjoin_proposal.process_res(response.content, request.client_response)
            
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
