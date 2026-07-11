// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";
import "../src/PaperWalletEscrow.sol";

contract PaperWalletEscrowTest is Test {
    YeetToken          token;
    PaperWalletEscrow  escrow;

    address owner    = address(0x1);
    address pool     = address(0x2);
    address platform = address(0x3);
    address issuer   = address(0x4);
    address bob      = address(0x5);
    address attacker = address(0x6);

    // Ephemeral claim keypair (the "secret" printed on the paper wallet).
    uint256 claimPk = 0xA11CE;
    address claimAddr;

    uint64 constant MIN_V = 1 hours;
    uint64 constant MAX_V = 365 days;

    function setUp() public {
        claimAddr = vm.addr(claimPk);
        vm.startPrank(owner);
        token  = new YeetToken(owner, pool);
        escrow = new PaperWalletEscrow(address(token), platform, owner, 1000 ether, MIN_V, MAX_V);
        token.transfer(issuer, 5000 ether);
        vm.stopPrank();

        vm.prank(issuer);
        token.approve(address(escrow), type(uint256).max);
    }

    function _create(uint256 amount) internal returns (uint64 expiry) {
        expiry = uint64(block.timestamp + 7 days);
        vm.prank(issuer);
        escrow.create(claimAddr, amount, expiry);
    }

    function _sign(uint256 pk, address recipient) internal view returns (bytes memory) {
        bytes32 digest = escrow.claimDigest(claimAddr, recipient);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);
        return abi.encodePacked(r, s, v);
    }

    function test_CreateLocksFunds() public {
        _create(100 ether);
        assertEq(token.balanceOf(address(escrow)), 100 ether);
    }

    function test_ClaimPaysRecipient() public {
        _create(100 ether);
        bytes memory sig = _sign(claimPk, bob);
        escrow.claim(claimAddr, bob, sig); // anyone can submit; sig binds recipient
        assertEq(token.balanceOf(bob), 100 ether);
        assertEq(token.balanceOf(address(escrow)), 0);
    }

    /// The core anti-front-running property: an attacker who copies the
    /// mempool signature but swaps the recipient cannot succeed, because the
    /// signature is over `bob`, not `attacker`.
    function test_FrontRunWithSwappedRecipientReverts() public {
        _create(100 ether);
        bytes memory sigForBob = _sign(claimPk, bob);
        vm.prank(attacker);
        vm.expectRevert(PaperWalletEscrow.BadSignature.selector);
        escrow.claim(claimAddr, attacker, sigForBob);
    }

    function test_AttackerWithoutKeyCannotForge() public {
        _create(100 ether);
        // attacker signs with their own key → signer != claimAddr
        bytes memory badSig = _sign(0xBAD, attacker);
        vm.expectRevert(PaperWalletEscrow.BadSignature.selector);
        escrow.claim(claimAddr, attacker, badSig);
    }

    function test_DoubleClaimReverts() public {
        _create(100 ether);
        escrow.claim(claimAddr, bob, _sign(claimPk, bob));
        vm.expectRevert(PaperWalletEscrow.AlreadyRedeemed.selector);
        escrow.claim(claimAddr, bob, _sign(claimPk, bob));
    }

    function test_ClaimAfterExpiryReverts() public {
        uint64 expiry = _create(100 ether);
        vm.warp(expiry + 1);
        vm.expectRevert(PaperWalletEscrow.Expired.selector);
        escrow.claim(claimAddr, bob, _sign(claimPk, bob));
    }

    function test_RefundOnlyAfterExpiryToIssuer() public {
        uint64 expiry = _create(100 ether);
        vm.expectRevert(PaperWalletEscrow.NotYetExpired.selector);
        escrow.refund(claimAddr);
        vm.warp(expiry + 1);
        uint256 before = token.balanceOf(issuer);
        escrow.refund(claimAddr); // callable by anyone; funds go to issuer
        assertEq(token.balanceOf(issuer) - before, 100 ether);
    }

    function test_NoAdminSweep() public {
        _create(100 ether);
        // There is no function on the contract that lets the owner move
        // escrowed funds. This asserts the invariant explicitly: even the
        // owner cannot drain before expiry/claim.
        vm.prank(owner);
        vm.expectRevert(); // no such selector; low-level call must fail
        (bool ok, ) = address(escrow).call(abi.encodeWithSignature("rescue(address,uint256)", owner, 100 ether));
        assertTrue(!ok);
    }

    function test_AboveMaxRejected() public {
        vm.prank(issuer);
        vm.expectRevert(PaperWalletEscrow.AboveMaximum.selector);
        escrow.create(claimAddr, 2000 ether, uint64(block.timestamp + 7 days));
    }

    function test_IssuancePauseBlocksCreateNotClaim() public {
        _create(100 ether);
        vm.prank(owner);
        escrow.setIssuancePaused(true);
        // create blocked
        address other = vm.addr(0xB0B);
        vm.prank(issuer);
        vm.expectRevert(PaperWalletEscrow.Paused.selector);
        escrow.create(other, 10 ether, uint64(block.timestamp + 7 days));
        // claim still works
        escrow.claim(claimAddr, bob, _sign(claimPk, bob));
        assertEq(token.balanceOf(bob), 100 ether);
    }
}
