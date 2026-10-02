// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";

contract YeetTokenTest is Test {
    YeetToken token;
    address owner = address(0x1);
    address alice = address(0x3);
    address bob   = address(0x4);

    function setUp() public {
        vm.prank(owner);
        token = new YeetToken(owner);
    }

    // ── Supply model (docs/mica/02 Teil J) ────────────────────────────────

    function test_MaxSupplyIs21Billion() public view {
        assertEq(token.MAX_SUPPLY(), 21_000_000_000 ether);
    }

    function test_InitialSupplyIsNonRewardTranches() public view {
        uint256 expected = (token.MAX_SUPPLY() * 25) / 100; // 10 % dev + 10 % team + 5 % reserve
        assertEq(token.totalSupply(), expected);
        assertEq(token.balanceOf(owner), expected);
    }

    function test_RewardReserveIs75Percent() public view {
        assertEq(token.REWARD_RESERVE(), (token.MAX_SUPPLY() * 75) / 100);
        assertEq(token.rewardsMinted(), 0);
        assertEq(token.rewardsRemaining(), token.REWARD_RESERVE());
        // Backend default YEET_CONVERSION_POOL mirrors this number.
        assertEq(token.REWARD_RESERVE(), 15_750_000_000 ether);
    }

    function test_TranchesSumToMaxSupply() public view {
        assertEq(
            token.DEVELOPER_SHARE() + token.TEAM_SHARE() + token.RESERVE_SHARE() + token.REWARD_RESERVE(),
            token.MAX_SUPPLY()
        );
    }

    // ── ERC-20 basics ─────────────────────────────────────────────────────

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

    // ── Reward minting ────────────────────────────────────────────────────

    function test_BatchMintRewards() public {
        address[] memory r  = new address[](3);
        uint256[] memory a  = new uint256[](3);
        string[]  memory s2 = new string[](3);
        r[0] = alice; a[0] = 5e18; s2[0] = "conversion";
        r[1] = bob;   a[1] = 1e18; s2[1] = "conversion";
        r[2] = alice; a[2] = 2e18; s2[2] = "conversion";
        uint256 before = token.totalSupply();
        vm.prank(owner);
        token.batchMintRewards(r, a, s2);
        assertEq(token.balanceOf(alice), 7e18);
        assertEq(token.balanceOf(bob), 1e18);
        assertEq(token.rewardsMinted(), 8e18);
        assertEq(token.rewardsRemaining(), token.REWARD_RESERVE() - 8e18);
        assertEq(token.totalSupply(), before + 8e18);
    }

    function test_BatchMintRewardsEmitsEvent() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = alice; a[0] = 3e18; s[0] = "conversion";
        vm.expectEmit(true, false, false, true);
        emit YeetToken.RewardMinted(alice, 3e18, "conversion");
        vm.prank(owner);
        token.batchMintRewards(r, a, s);
    }

    function test_FullReserveReachesExactlyMaxSupply() public {
        _mintReward(alice, token.REWARD_RESERVE());
        assertEq(token.totalSupply(), token.MAX_SUPPLY());
        assertEq(token.rewardsRemaining(), 0);
    }

    function test_BatchMintRewardsCappedAtReserve() public {
        _mintReward(alice, token.REWARD_RESERVE() - 1);
        // 2 wei more than remaining → whole batch reverts, nothing minted.
        address[] memory r = new address[](2);
        uint256[] memory a = new uint256[](2);
        string[]  memory s = new string[](2);
        r[0] = bob; a[0] = 1; s[0] = "conversion";
        r[1] = bob; a[1] = 1; s[1] = "conversion";
        vm.prank(owner);
        vm.expectRevert("Exceeds reward reserve");
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(bob), 0);
        assertEq(token.rewardsRemaining(), 1);
    }

    function test_BurnDoesNotReopenReserve() public {
        _mintReward(alice, token.REWARD_RESERVE());
        vm.prank(alice);
        token.burn(1_000 ether);
        assertLt(token.totalSupply(), token.MAX_SUPPLY());
        // Reserve is tracked by rewardsMinted, not by totalSupply headroom.
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1; s[0] = "conversion";
        vm.prank(owner);
        vm.expectRevert("Exceeds reward reserve");
        token.batchMintRewards(r, a, s);
    }

    function test_OnlyOwnerCanBatchMint() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1 ether; s[0] = "conversion";
        vm.prank(alice);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, alice));
        token.batchMintRewards(r, a, s);
    }

    function test_BatchMintRewardsMismatchReverts() public {
        address[] memory r = new address[](2);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](2);
        r[0] = alice; r[1] = bob; a[0] = 1e18; s[0] = "x"; s[1] = "y";
        vm.prank(owner);
        vm.expectRevert("Length mismatch");
        token.batchMintRewards(r, a, s);
    }

    function test_BatchMintRewardsMaxBatch() public {
        uint256 n = token.MAX_BATCH() + 1;
        address[] memory r = new address[](n);
        uint256[] memory a = new uint256[](n);
        string[]  memory s = new string[](n);
        for (uint256 i = 0; i < n; i++) { r[i] = alice; a[i] = 1; s[i] = "c"; }
        vm.prank(owner);
        vm.expectRevert("Max 200 per batch");
        token.batchMintRewards(r, a, s);
    }

    // ── Ownership hand-over (multisig) ────────────────────────────────────

    function test_OwnershipIsTwoStep() public {
        vm.prank(owner);
        token.transferOwnership(alice);
        assertEq(token.owner(), owner);          // not yet
        assertEq(token.pendingOwner(), alice);
        vm.prank(alice);
        token.acceptOwnership();
        assertEq(token.owner(), alice);
    }

    // ── helpers ───────────────────────────────────────────────────────────

    function _mintReward(address to, uint256 amount) internal {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = to; a[0] = amount; s[0] = "conversion";
        vm.prank(owner);
        token.batchMintRewards(r, a, s);
    }
}
