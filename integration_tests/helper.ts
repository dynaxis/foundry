import { SDK } from "../";
import { Transaction } from "../src/core/classes";

export const CODECHAIN_NETWORK_ID = process.env.CODECHAIN_NETWORK_ID || "tc";
export const SERVER_URL =
    process.env.CODECHAIN_RPC_HTTP || "http://localhost:8080";
export const sdk = new SDK({
    server: SERVER_URL,
    keyStoreType: "memory",
    networkId: CODECHAIN_NETWORK_ID
});

export const ACCOUNT_SECRET =
    process.env.ACCOUNT_SECRET ||
    "ede1d4ccb4ec9a8bbbae9a13db3f4a7b56ea04189be86ac3a6a439d9a0a1addd";
export const ACCOUNT_ID =
    process.env.ACCOUNT_ID ||
    sdk.util.getAccountIdFromPrivate(ACCOUNT_SECRET).toString(); // "0x6fe64ffa3a46c074226457c90ccb32dc06ccced1"
export const ACCOUNT_ADDRESS =
    process.env.ACCOUNT_ADDRESS ||
    sdk.core.classes.PlatformAddress.fromAccountId(ACCOUNT_ID).toString(); // "tccq9h7vnl68frvqapzv3tujrxtxtwqdnxw6yamrrgd"
export const ACCOUNT_PASSPHRASE = process.env.ACCOUNT_PASSPHRASE || "satoshi";

export const sendTransaction = async ({
    transaction
}: {
    transaction: Transaction;
}) => {
    const parcel = sdk.core.createAssetTransactionParcel({
        transaction
    });
    const signedParcel = parcel.sign({
        secret: ACCOUNT_SECRET,
        seq: await sdk.rpc.chain.getSeq(ACCOUNT_ADDRESS),
        fee: 10
    });
    const parcelHash = await sdk.rpc.chain.sendSignedParcel(signedParcel);
    return {
        parcelHash
    };
};

export const mintAsset = async ({
    metadata,
    amount,
    lockScriptHash,
    registrar
}: any) => {
    const assetScheme = sdk.core.createAssetScheme({
        shardId: 0,
        metadata,
        amount,
        registrar
    });
    const assetAddress = sdk.core.classes.AssetTransferAddress.fromTypeAndPayload(
        0,
        lockScriptHash
    );
    const assetMintTransaction = assetScheme.createMintTransaction({
        recipient: assetAddress
    });
    return {
        ...(await sendTransaction({ transaction: assetMintTransaction })),
        assetMintTransaction
    };
};

export const payment = async (params?: { inc_seq?: number }) => {
    const { inc_seq = 0 } = params || {};
    let seq = await sdk.rpc.chain.getSeq(ACCOUNT_ADDRESS);
    for (let i = 0; i < inc_seq; i++) {
        seq = seq.increase();
    }
    const p = sdk.core
        .createPaymentParcel({
            amount: 10,
            recipient: ACCOUNT_ADDRESS
        })
        .sign({
            secret: ACCOUNT_SECRET,
            fee: 10,
            seq
        });
    return await sdk.rpc.chain.sendSignedParcel(p);
};
