// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";

contract YeetTokenTest is Test {
    YeetToken token;
    address owner    = address(0x1);
    address pool     = address(0x2);
    address alice    = address(0x3);
    address bob      = address(0x4);

    function setUp() public {
        vm.prank(owner);
        token = new YeetToken(owner, pool);
    }

    function test_TotalSupply() public view {
        assertEq(token.totalSupply(), token.MAX_SUPPLY());
    }

    function test_OwnerBalance() public view {
        uint256 expected = (token.MAX_SUPPLY() * 70) / 100; // 40+20+10
        assertEq(token.balanceOf(owner), expected);
    }

    function test_PoolBalance() public view {
        uint256 expected = (token.MAX_SUPPLY() * 30) / 100;
        assertEq(token.balanceOf(pool), expected);
    }

    function test_Transfer() public {
        vm.prank(owner);
        token.transfer(alice, 1000 ether);
        assertEq(token.balanceOf(alice), 1000 ether);
    }

    function test_Burn() public {
        vm.startPrank(owner);
        uint256 before = token.totalSupply();
        token.burn(100 ether);
        assertEq(token.totalSupply(), before - 100 ether);
        vm.stopPrank();
    }

    function test_MintCappedAtMax() public {
        vm.prank(owner);
        vm.expectRevert("Exceeds max supply");
        token.mint(alice, 1);
    }

    function test_SetRewardPool() public {
        vm.prank(owner);
        token.setRewardPool(alice);
        assertEq(token.rewardPool(), alice);
    }

    function test_OnlyOwnerCanMint() public {
        vm.prank(alice);
        vm.expectRevert();
        token.mint(bob, 1 ether);
    }
}