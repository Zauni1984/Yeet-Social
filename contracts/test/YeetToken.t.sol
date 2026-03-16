// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";
import "../src/YeetPlatform.sol";
import "../src/YeetSwapVault.sol";

contract YeetTokenTest is Test {
    YeetToken   token;
    YeetPlatform platform;
    YeetSwapVault vault;

    address owner     = address(this);
    address treasury  = makeAddr("treasury");
    address team      = makeAddr("team");
    address liquidity = makeAddr("liquidity");
    address community = makeAddr("community");
    address oracle    = makeAddr("oracle");
    address alice     = makeAddr("alice");
    address bob       = makeAddr("bob");

    function setUp() public {
        token    = new YeetToken(treasury, team, liquidity, community);
        platform = new YeetPlatform(address(token));
        vault    = new YeetSwapVault(address(token), oracle);

        // Fund platform with reward pool
        token.setRewardDistributor(address(platform));
        token.setSwapVault(address(vault));
    }

    function test_TotalSupply() public view {
        assertEq(token.totalSupply(), 21_000_000_000 * 1e18);
    }

    function test_Allocations() public view {
        assertEq(token.balanceOf(treasury),  token.TREASURY());
        assertEq(token.balanceOf(team),      token.TEAM_VESTING());
        assertEq(token.balanceOf(liquidity), token.LIQUIDITY());
        assertEq(token.balanceOf(community), token.COMMUNITY());
    }

    function test_PostReward() public {
        uint256 before = token.balanceOf(alice);
        platform.rewardPost(alice);
        assertEq(token.balanceOf(alice), before + 5 * 1e18);
    }

    function test_BurnToDead() public {
        uint256 aliceAmt = 100 * 1e18;
        deal(address(token), alice, aliceAmt);
        vm.prank(alice);
        token.burnToDead(50 * 1e18);
        assertEq(token.burnedBalance(), 50 * 1e18);
        assertEq(token.balanceOf(alice), 50 * 1e18);
    }

    function test_Tip() public {
        uint256 tipAmt = 100 * 1e18;
        deal(address(token), alice, tipAmt);
        vm.startPrank(alice);
        token.approve(address(platform), tipAmt);
        platform.tip(bob, tipAmt);
        vm.stopPrank();
        // Bob gets 95%, treasury gets 5%
        assertApproxEqRel(token.balanceOf(bob), 95 * 1e18, 1e15);
    }

    function test_SwapClaim() public {
        uint256 swapAmt = 1000 * 1e18;
        bytes32 oldTx   = keccak256("old_chain_tx_1");

        // Create oracle signature
        (address oracleSigner, uint256 oracleKey) = makeAddrAndKey("oracle");
        vm.prank(owner);
        vault.setOracle(oracleSigner);

        bytes32 hash = keccak256(abi.encodePacked(alice, swapAmt, oldTx));
        bytes32 ethHash = keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", hash));
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(oracleKey, ethHash);
        bytes memory sig = abi.encodePacked(r, s, v);

        deal(address(token), address(vault), swapAmt);
        vault.claim(alice, swapAmt, oldTx, sig);
        assertEq(token.balanceOf(alice), swapAmt);
    }
}