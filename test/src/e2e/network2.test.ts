// Copyright 2018-2019 Kodebox, Inc.
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
import "mocha";
import { wait } from "../helper/promise";
import CodeChain from "../helper/spawn";

describe("network2 nodes", function() {
    let nodeA: CodeChain;
    let nodeB: CodeChain;
    const address = "127.0.0.1";
    before(async function() {
        nodeA = new CodeChain();
        nodeB = new CodeChain();
        await Promise.all([nodeA.start(), nodeB.start()]);
    });

    describe("Not connected", function() {
        beforeEach(async function() {
            this.timeout(60_000);
            // ensure disconnected
            if (
                !(await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                }))
            ) {
                return;
            }
            await nodeA.rpc.net.disconnect({
                address: address.toString(),
                port: nodeB.port
            });
            while (
                await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                })
            ) {
                await wait(500);
            }
        });

        it("connect", async function() {
            expect(
                await nodeA.rpc.net.connect({
                    address: address.toString(),
                    port: nodeB.port
                })
            ).not.throw;

            while (
                !(await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                }))
            ) {
                await wait(500);
            }
        });

        it("getPeerCount", async function() {
            expect(await nodeA.rpc.net.getPeerCount()).to.equal(0);
        });

        it("getPeers", async function() {
            expect(await nodeA.rpc.net.getEstablishedPeers()).to.be.empty;
        });
    });

    describe("1 connected", function() {
        beforeEach(async function() {
            this.timeout(60_000);
            // ensure connected
            if (
                await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                })
            ) {
                return;
            }
            await nodeA.rpc.net.connect({
                address: address.toString(),
                port: nodeB.port
            });
            while (
                !(await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                }))
            ) {
                await wait(500);
            }
        });

        it("isConnected", async function() {
            expect(
                await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                })
            ).to.be.true;
        });

        it("disconnect", async function() {
            expect(
                await nodeA.rpc.net.disconnect({
                    address: address.toString(),
                    port: nodeB.port
                })
            ).not.throw;

            while (
                await nodeA.rpc.net.isConnected({
                    address: address.toString(),
                    port: nodeB.port
                })
            ) {
                await wait(500);
            }
        });

        it("getPeerCount", async function() {
            expect(await nodeA.rpc.net.getPeerCount()).to.equal(1);
            expect(await nodeB.rpc.net.getPeerCount()).to.equal(1);
        });

        it("getPeers", async function() {
            expect(await nodeA.rpc.net.getEstablishedPeers()).to.deep.equal([
                `${address}:${nodeB.port}`
            ]);
            expect(await nodeB.rpc.net.getEstablishedPeers()).to.deep.equal([
                `${address}:${nodeA.port}`
            ]);
        });
    });

    afterEach(function() {
        if (this.currentTest!.state === "failed") {
            nodeA.keepLogs();
            nodeB.keepLogs();
        }
    });

    after(async function() {
        await Promise.all([nodeA.clean(), nodeB.clean()]);
    });
});
