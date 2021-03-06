use crate::chain::client::Client;
use crate::core::bitcoin::bitcoin;
use crate::core::primitives::transaction::Sighash;
use crate::core::primitives::Result;
use crate::core::signatory_set::SignatorySet;
use bech32::{FromBase32, ToBase32};
use failure::bail;
use log::info;
use secp256k1::{Secp256k1, SecretKey, SignOnly};
use std::fs;
use std::path::Path;

const ADDRESS_PREFIX: &str = "nomic";

pub struct Wallet {
    privkey: secp256k1::SecretKey,
    secp: Secp256k1<SignOnly>,
}

impl Wallet {
    pub fn load_or_generate<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        let privkey_bytes = if path.exists() {
            info!("Loading existing wallet from {:?}", path);
            fs::read(path)?
        } else {
            info!("Generating new wallet at {:?}", path);
            let bytes: [u8; 32] = rand::random();
            fs::write(path, bytes)?;
            bytes.to_vec()
        };

        let privkey = SecretKey::from_slice(privkey_bytes.as_slice())?;
        let secp = Secp256k1::signing_only();

        Ok(Wallet { privkey, secp })
    }

    pub fn pubkey(&self) -> bitcoin::PublicKey {
        let secp = secp256k1::Secp256k1::signing_only();
        let key = secp256k1::PublicKey::from_secret_key(&secp, &self.privkey);
        bitcoin::PublicKey {
            compressed: true,
            key,
        }
    }

    pub fn deposit_address(&self, signatories: &SignatorySet) -> bitcoin::Address {
        let script = crate::core::signatory_set::redeem_script(signatories, self.pubkey_bytes());
        bitcoin::Address::p2wsh(&script, bitcoin::Network::Testnet)
    }

    pub fn pubkey_bytes(&self) -> Vec<u8> {
        self.pubkey().to_bytes()
    }

    pub fn receive_address(&self) -> String {
        bech32::encode(ADDRESS_PREFIX, self.pubkey_bytes().to_base32()).unwrap()
    }

    pub fn send(&self, client: &mut Client, address: &str, amount: u64) -> Result<()> {
        use crate::core::primitives::transaction::{Transaction, TransferTransaction};

        let sender_address = self.pubkey_bytes();

        let (prefix, receiver_address_u5) = bech32::decode(address)?;
        if prefix != ADDRESS_PREFIX {
            bail!("Invalid address prefix");
        }
        let receiver_address = Vec::from_base32(&receiver_address_u5)?;

        let account = client.get_account(sender_address.as_slice())?;
        let mut tx = TransferTransaction {
            to: receiver_address,
            from: sender_address,
            signature: vec![],
            fee_amount: 1000,
            nonce: account.nonce,
            amount: amount,
        };

        let message = secp256k1::Message::from_slice(tx.sighash()?.as_slice()).unwrap();
        let signature = self.secp.sign(&message, &self.privkey);
        tx.signature = signature.serialize_compact().to_vec();

        client.send(Transaction::Transfer(tx))?;
        Ok(())
    }

    pub fn withdraw(&self, client: &mut Client, bitcoin_address: &str, amount: u64) -> Result<()> {
        use crate::core::primitives::transaction::{Transaction, WithdrawalTransaction};

        let address: bitcoin::Address = bitcoin_address.parse()?;
        let script = address.script_pubkey();

        let sender_address = self.pubkey_bytes();
        let account = client.get_account(sender_address.as_slice())?;

        let mut tx = WithdrawalTransaction {
            from: sender_address,
            to: script,
            amount,
            signature: vec![],
            nonce: account.nonce,
        };

        let message = secp256k1::Message::from_slice(tx.sighash()?.as_slice()).unwrap();
        let signature = self.secp.sign(&message, &self.privkey);
        tx.signature = signature.serialize_compact().to_vec();

        client.send(Transaction::Withdrawal(tx))?;
        Ok(())
    }
}
