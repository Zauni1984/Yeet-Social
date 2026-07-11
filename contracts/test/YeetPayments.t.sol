// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";
import "../src/YeetPayments.sol";

contract YeetPaymentsTest is Test {
    YeetToken    token;
    YeetPayments pay;

    address owner    = address(0x1);
    address pool     = address(0x2);
    address platform = address(0x3);
    address alice    = address(0x4);
    address bob      = address(0x5);

    bytes32 REF = keccak256("post-uuid-1234");

    function setUp() public {
        vm.startPrank(owner);
        token = new YeetToken(owner, pool);
        pay   = new YeetPayments(address(token), platform, owner);
        token.transfer(alice, 1000 ether);
        vm.stopPrank();

        vm.prank(alice);
        token.approve(address(pay), type(uint256).max);
    }

    function test_TipSplits90_10() public {
        uint256 amt = 100 ether;
        vm.prank(alice);
        pay.tip(bob, REF, amt);
        assertEq(token.balanceOf(bob), 90 ether);
        assertEq(token.balanceOf(platform), 10 ether);
        assertEq(pay.totalReceived(bob), 90 ether);
    }

    function test_PayPerViewGoesToCreator() public {
        vm.prank(alice);
        pay.pay(YeetPayments.Kind.PayPerView, bob, REF, 50 ether);
        assertEq(token.balanceOf(bob), 45 ether);
        assertEq(token.balanceOf(platform), 5 ether);
    }

    function test_RevertSelfPayment() public {
        vm.prank(alice);
        vm.expectRevert(YeetPayments.SelfPayment.selector);
        pay.tip(alice, REF, 10 ether);
    }

    function test_RevertBelowMinimum() public {
        vm.prank(alice);
        vm.expectRevert(YeetPayments.BelowMinimum.selector);
        pay.tip(bob, REF, 0.5 ether);
    }

    function test_ContractNeverHoldsFunds() public {
        vm.prank(alice);
        pay.tip(bob, REF, 100 ether);
        assertEq(token.balanceOf(address(pay)), 0, "payments contract must never hold YEET");
    }

    function test_PausedBlocksNewPayments() public {
        vm.prank(owner);
        pay.pause();
        vm.prank(alice);
        vm.expectRevert();
        pay.tip(bob, REF, 100 ether);
    }

    function test_ZeroFeeWhenBpsZero() public {
        vm.prank(owner);
        pay.setPlatformFee(0);
        vm.prank(alice);
        pay.tip(bob, REF, 100 ether);
        assertEq(token.balanceOf(bob), 100 ether);
        assertEq(token.balanceOf(platform), 0);
    }

    function testFuzz_SplitConserves(uint96 amount) public {
        amount = uint96(bound(uint256(amount), 1 ether, 1000 ether));
        vm.prank(alice);
        pay.tip(bob, REF, amount);
        assertEq(token.balanceOf(bob) + token.balanceOf(platform), amount);
    }
}
