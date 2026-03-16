// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";
import "../src/YeetTipping.sol";

contract YeetTippingTest is Test {
    YeetToken   token;
    YeetTipping tipping;

    address owner    = address(0x1);
    address pool     = address(0x2);
    address platform = address(0x3);
    address alice    = address(0x4);
    address bob      = address(0x5);

    bytes32 POST_ID = keccak256("post-uuid-1234");

    function setUp() public {
        vm.startPrank(owner);
        token   = new YeetToken(owner, pool);
        tipping = new YeetTipping(address(token), platform, owner);

        // Fund alice with 1000 YEET
        token.transfer(alice, 1000 ether);
        vm.stopPrank();

        // Alice approves tipping contract
        vm.prank(alice);
        token.approve(address(tipping), type(uint256).max);
    }

    function test_TipSendsNetToBob() public {
        uint256 tipAmt = 100 ether;
        uint256 fee    = (tipAmt * 1000) / 10000; // 10%
        uint256 net    = tipAmt - fee;

        vm.prank(alice);
        tipping.tip(bob, POST_ID, tipAmt);

        assertEq(token.balanceOf(bob),      net);
        assertEq(token.balanceOf(platform), fee);
    }

    function test_CannotTipSelf() public {
        vm.prank(alice);
        vm.expectRevert("Cannot tip yourself");
        tipping.tip(alice, POST_ID, 100 ether);
    }

    function test_BelowMinimumReverts() public {
        vm.prank(alice);
        vm.expectRevert("Below minimum tip");
        tipping.tip(bob, POST_ID, 0.5 ether);
    }

    function test_PostTipsAccumulate() public {
        vm.prank(alice);
        tipping.tip(bob, POST_ID, 10 ether);
        vm.prank(alice);
        tipping.tip(bob, POST_ID, 20 ether);

        assertEq(tipping.postTips(POST_ID), 30 ether);
    }

    function test_PauseBlocksTipping() public {
        vm.prank(owner);
        tipping.pause();

        vm.prank(alice);
        vm.expectRevert();
        tipping.tip(bob, POST_ID, 10 ether);
    }

    function test_SetFeeAboveMaxReverts() public {
        vm.prank(owner);
        vm.expectRevert("Fee too high");
        tipping.setPlatformFee(2001);
    }
}