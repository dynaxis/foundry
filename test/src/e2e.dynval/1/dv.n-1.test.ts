// Copyright 2019 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

import { expect } from "chai";
import RPC from "foundry-rpc";
import "mocha";
import * as stake from "../../stakeholder";

import { H256 } from "../../primitives/src";
import { validators as originalValidators } from "../../../tendermint.dynval/constants";
import { faucetAddress, faucetSecret } from "../../helper/constants";
import { PromiseExpect } from "../../helper/promise";
import { setTermTestTimeout, withNodes } from "../setup";

const allDynValidators = originalValidators.slice(0, 4);
const [alice, ...otherDynValidators] = allDynValidators;

describe("Dynamic Validator N -> N-1", function() {
    const promiseExpect = new PromiseExpect();

    async function aliceContainedCheck(rpc: RPC) {
        const blockNumber = await rpc.chain.getBestBlockNumber();
        const termMedata = await stake.getTermMetadata(rpc, blockNumber);
        const currentTermInitialBlockNumber =
            termMedata!.lastTermFinishedBlockNumber + 1;
        const validatorsBefore = (await stake.getPossibleAuthors(
            rpc,
            currentTermInitialBlockNumber
        ))!.map(platformAddr => platformAddr.toString());

        expect(termMedata!.currentTermId).to.be.equals(1);
        expect(validatorsBefore.length).to.be.equals(allDynValidators.length);
        expect(validatorsBefore).to.includes(alice.address.toString());
        expect(validatorsBefore).contains.all.members(
            allDynValidators.map(validator => validator.address.value)
        );
    }

    async function aliceDropOutCheck(rpc: RPC) {
        const blockNumber = await rpc.chain.getBestBlockNumber();
        const termMedata = await stake.getTermMetadata(rpc, blockNumber);
        const currentTermInitialBlockNumber =
            termMedata!.lastTermFinishedBlockNumber + 1;
        const validatorsAfter = (await stake.getPossibleAuthors(
            rpc,
            currentTermInitialBlockNumber
        ))!.map(platformAddr => platformAddr.toString());

        expect(termMedata!.currentTermId).to.be.equals(2);
        expect(validatorsAfter.length).to.be.equals(otherDynValidators.length);
        expect(validatorsAfter).not.to.includes(alice.address.toString());
        expect(validatorsAfter).contains.all.members(
            otherDynValidators.map(validator => validator.address.value)
        );
    }

    describe("A node is imprisoned to jail", async function() {
        const { nodes } = withNodes(this, {
            promiseExpect,
            validators: allDynValidators.map((signer, index) => ({
                signer,
                delegation: 5_000,
                deposit: 10_000_000 - index // tie-breaker
            })),
            onBeforeEnable: async allDynNodes => {
                const aliceNode = allDynNodes[0];
                // Kill the alice node first to make alice not to participate in the term 1.
                await aliceNode.clean();
            }
        });

        it("alice should be dropped out from validator list", async function() {
            const termWaiter = setTermTestTimeout(this, {
                terms: 1
            });

            const checkingNode = nodes[1];
            await aliceContainedCheck(checkingNode.rpc);

            await termWaiter.waitNodeUntilTerm(checkingNode, {
                target: 2,
                termPeriods: 1
            });

            await aliceDropOutCheck(checkingNode.rpc);
        });
    });

    describe("A node dropped out of validator list by revoke action", async function() {
        const { nodes } = withNodes(this, {
            promiseExpect,
            validators: allDynValidators.map((signer, index) => ({
                signer,
                delegation: 5_000,
                deposit: 10_000_000 - index // tie-breaker
            }))
        });

        it("Revoke all delegation deposits from Alice", async function() {
            const termWaiter = setTermTestTimeout(this, {
                terms: 1
            });
            const checkingNode = nodes[1];
            await aliceContainedCheck(checkingNode.rpc);

            const faucetSeq = (await checkingNode.rpc.chain.getSeq({
                address: faucetAddress.toString()
            }))!;
            // Revoke all the delegation deposits
            const tx = checkingNode.testFramework.core
                .createRevokeTransaction({
                    delegatee: alice.address,
                    quantity: 5_000
                })
                .sign({
                    secret: faucetSecret,
                    seq: faucetSeq,
                    fee: 10
                });
            const revokeTx = await checkingNode.rpc.mempool.sendSignedTransaction(
                {
                    tx: tx.rlpBytes().toString("hex")
                }
            );
            await checkingNode.waitForTx(new H256(revokeTx));

            await termWaiter.waitNodeUntilTerm(checkingNode, {
                target: 2,
                termPeriods: 1
            });
            await aliceDropOutCheck(checkingNode.rpc);
        });

        it("Revoke delegation deposits to make it be under threshold", async function() {
            const termWaiter = setTermTestTimeout(this, {
                terms: 1
            });
            const checkingNode = nodes[1];
            await aliceContainedCheck(checkingNode.rpc);

            const faucetSeq = (await checkingNode.rpc.chain.getSeq({
                address: faucetAddress.toString()
            }))!;
            // make remaining deposits under threshold.
            const tx = checkingNode.testFramework.core
                .createRevokeTransaction({
                    delegatee: alice.address,
                    quantity: 4_500
                })
                .sign({
                    secret: faucetSecret,
                    seq: faucetSeq,
                    fee: 10
                });
            const revokeTx = await checkingNode.rpc.mempool.sendSignedTransaction(
                {
                    tx: tx.rlpBytes().toString("hex")
                }
            );
            await checkingNode.waitForTx(new H256(revokeTx));

            await termWaiter.waitNodeUntilTerm(checkingNode, {
                target: 2,
                termPeriods: 1
            });
            await aliceDropOutCheck(checkingNode.rpc);
        });
    });

    afterEach(async function() {
        promiseExpect.checkFulfilled();
    });
});
