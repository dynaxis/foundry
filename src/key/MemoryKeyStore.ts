import * as _ from "lodash";

import { H256 } from "../core/H256";
import {
    blake256,
    generatePrivateKey,
    getAccountIdFromPublic,
    getPublicFromPrivate,
    signEcdsa
} from "../utils";
import { KeyManagementAPI, KeyStore } from "./KeyStore";

/**
 * @hidden
 */

class KeyManager implements KeyManagementAPI {
    private privateKeyMap: { [key: string]: string } = {};
    private passphraseMap: { [key: string]: string } = {};
    private publicKeyMap: { [key: string]: string };
    private mappingKeyMaker: (value: string) => string;

    public constructor(keyMaker: (value: string) => string) {
        this.publicKeyMap = {};
        this.mappingKeyMaker = keyMaker;
    }

    public getKeyList(): Promise<string[]> {
        return Promise.resolve(_.keys(this.privateKeyMap));
    }

    public createKey(params: { passphrase?: string } = {}): Promise<string> {
        const privateKey = generatePrivateKey();
        const publicKey = getPublicFromPrivate(privateKey);
        const key = this.mappingKeyMaker(publicKey);
        this.privateKeyMap[key] = privateKey;
        this.passphraseMap[key] = params.passphrase || "";
        this.publicKeyMap[key] = publicKey;
        return Promise.resolve(key);
    }

    public removeKey(params: { key: string }): Promise<boolean> {
        const { key } = params;
        if (this.privateKeyMap[key]) {
            delete this.privateKeyMap[key];
            delete this.publicKeyMap[key];
            delete this.passphraseMap[key];
            return Promise.resolve(true);
        } else {
            return Promise.resolve(false);
        }
    }

    public getPublicKey(params: { key: string }): Promise<string | null> {
        const { key } = params;
        if (this.publicKeyMap[key]) {
            return Promise.resolve(this.publicKeyMap[key]);
        } else {
            return Promise.resolve(null);
        }
    }

    public sign(params: {
        key: string;
        message: string;
        passphrase?: string;
    }): Promise<string> {
        const { key, message, passphrase = "" } = params;
        if (passphrase !== this.passphraseMap[key]) {
            return Promise.reject("The passphrase does not match");
        }
        const { r, s, v } = signEcdsa(message, this.privateKeyMap[key]);
        const sig = `${_.padStart(r, 64, "0")}${_.padStart(
            s,
            64,
            "0"
        )}${_.padStart(v.toString(16), 2, "0")}`;
        return Promise.resolve(sig);
    }
}

export class MemoryKeyStore implements KeyStore {
    public platform = new KeyManager(getAccountIdFromPublic);
    public asset = new KeyManager(this.getHash);

    private getHash(publicKey: string): string {
        return H256.ensure(blake256(publicKey)).value;
    }
}
