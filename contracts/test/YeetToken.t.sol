// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetToken.sol";

contract YeetTokenTest is Test {
    YeetToken token;
    address owner  = address(0x1);
    address minter = address(0x2);
    address alice  = address(0x3);
    address bob    = address(0x4);
    uint256 constant CAP = 1_000_000 ether; // daily cap used in these tests

    function setUp() public {
        vm.warp(1_700_000_000); // a fixed "now" so day boundaries are deterministic
        vm.prank(owner);
        token = new YeetToken(owner, minter, CAP);
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

    function test_ConstructorSetsRoles() public view {
        assertEq(token.owner(), owner);
        assertEq(token.minter(), minter);
        assertEq(token.dailyMintCap(), CAP);
        assertEq(token.dailyRemaining(), CAP);
        assertFalse(token.mintingPaused());
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

    // ── Reward minting: who ───────────────────────────────────────────────

    function test_MinterCanBatchMint() public {
        address[] memory r  = new address[](3);
        uint256[] memory a  = new uint256[](3);
        string[]  memory s2 = new string[](3);
        r[0] = alice; a[0] = 5e18; s2[0] = "conversion";
        r[1] = bob;   a[1] = 1e18; s2[1] = "conversion";
        r[2] = alice; a[2] = 2e18; s2[2] = "conversion";
        uint256 before = token.totalSupply();
        vm.prank(minter);
        token.batchMintRewards(r, a, s2);
        assertEq(token.balanceOf(alice), 7e18);
        assertEq(token.balanceOf(bob), 1e18);
        assertEq(token.rewardsMinted(), 8e18);
        assertEq(token.rewardsRemaining(), token.REWARD_RESERVE() - 8e18);
        assertEq(token.totalSupply(), before + 8e18);
        assertEq(token.dailyRemaining(), CAP - 8e18);
    }

    function test_OwnerCannotMint() public {
        // The owner (multisig) governs; it never mints itself.
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1 ether; s[0] = "conversion";
        vm.prank(owner);
        vm.expectRevert(YeetToken.NotMinter.selector);
        token.batchMintRewards(r, a, s);
    }

    function test_StrangerCannotMint() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1 ether; s[0] = "conversion";
        vm.prank(alice);
        vm.expectRevert(YeetToken.NotMinter.selector);
        token.batchMintRewards(r, a, s);
    }

    function test_OwnerRotatesMinter() public {
        vm.prank(owner);
        token.setMinter(bob);
        assertEq(token.minter(), bob);
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = alice; a[0] = 1 ether; s[0] = "conversion";
        vm.prank(minter); // old key is dead
        vm.expectRevert(YeetToken.NotMinter.selector);
        token.batchMintRewards(r, a, s);
        vm.prank(bob);
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(alice), 1 ether);
    }

    function test_OnlyOwnerManagesRoles() public {
        vm.startPrank(minter);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, minter));
        token.setMinter(minter);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, minter));
        token.setDailyMintCap(0);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, minter));
        token.setMintingPaused(false);
        vm.stopPrank();
    }

    // ── Reward minting: bounds ────────────────────────────────────────────

    function test_DailyCapEnforcedAndResetsNextDay() public {
        _mintReward(alice, CAP); // exactly the cap
        assertEq(token.dailyRemaining(), 0);
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1; s[0] = "conversion";
        vm.prank(minter);
        vm.expectRevert(abi.encodeWithSelector(YeetToken.DailyCapExceeded.selector, uint256(1), uint256(0)));
        token.batchMintRewards(r, a, s);
        // Next UTC day: cap is fresh again.
        vm.warp(block.timestamp + 1 days);
        assertEq(token.dailyRemaining(), CAP);
        vm.prank(minter);
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(bob), 1);
    }

    function test_DailyCapRevertIsAtomic() public {
        _mintReward(alice, CAP - 10);
        address[] memory r = new address[](2);
        uint256[] memory a = new uint256[](2);
        string[]  memory s = new string[](2);
        r[0] = bob; a[0] = 5; s[0] = "c";
        r[1] = bob; a[1] = 6; s[1] = "c"; // total 11 > 10 remaining
        vm.prank(minter);
        vm.expectRevert(abi.encodeWithSelector(YeetToken.DailyCapExceeded.selector, uint256(11), uint256(10)));
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(bob), 0);
        assertEq(token.rewardsMinted(), CAP - 10);
    }

    function test_NoCapWhenZero() public {
        vm.prank(owner);
        token.setDailyMintCap(0);
        assertEq(token.dailyRemaining(), type(uint256).max);
        _mintReward(alice, 5_000_000 ether); // far above the previous cap
        assertEq(token.balanceOf(alice), 5_000_000 ether);
    }

    function test_PauseStopsMintingNotTransfers() public {
        vm.prank(owner);
        token.setMintingPaused(true);
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1 ether; s[0] = "conversion";
        vm.prank(minter);
        vm.expectRevert(YeetToken.MintingIsPaused.selector);
        token.batchMintRewards(r, a, s);
        vm.prank(owner);
        token.transfer(alice, 1 ether); // transfers unaffected
        assertEq(token.balanceOf(alice), 1 ether);
        vm.prank(owner);
        token.setMintingPaused(false);
        vm.prank(minter);
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(bob), 1 ether);
    }

    function test_FullReserveReachesExactlyMaxSupply() public {
        vm.prank(owner);
        token.setDailyMintCap(0);
        _mintReward(alice, token.REWARD_RESERVE());
        assertEq(token.totalSupply(), token.MAX_SUPPLY());
        assertEq(token.rewardsRemaining(), 0);
    }

    function test_BatchMintRewardsCappedAtReserve() public {
        vm.prank(owner);
        token.setDailyMintCap(0);
        _mintReward(alice, token.REWARD_RESERVE() - 1);
        address[] memory r = new address[](2);
        uint256[] memory a = new uint256[](2);
        string[]  memory s = new string[](2);
        r[0] = bob; a[0] = 1; s[0] = "conversion";
        r[1] = bob; a[1] = 1; s[1] = "conversion";
        vm.prank(minter);
        vm.expectRevert("Exceeds reward reserve");
        token.batchMintRewards(r, a, s);
        assertEq(token.balanceOf(bob), 0);
        assertEq(token.rewardsRemaining(), 1);
    }

    function test_BurnDoesNotReopenReserve() public {
        vm.prank(owner);
        token.setDailyMintCap(0);
        _mintReward(alice, token.REWARD_RESERVE());
        vm.prank(alice);
        token.burn(1_000 ether);
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = bob; a[0] = 1; s[0] = "conversion";
        vm.prank(minter);
        vm.expectRevert("Exceeds reward reserve");
        token.batchMintRewards(r, a, s);
    }

    function test_ZeroRecipientReverts() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = address(0); a[0] = 1 ether; s[0] = "conversion";
        vm.prank(minter);
        vm.expectRevert("Zero recipient");
        token.batchMintRewards(r, a, s);
    }

    function test_BatchMintRewardsMismatchReverts() public {
        address[] memory r = new address[](2);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](2);
        r[0] = alice; r[1] = bob; a[0] = 1e18; s[0] = "x"; s[1] = "y";
        vm.prank(minter);
        vm.expectRevert("Length mismatch");
        token.batchMintRewards(r, a, s);
    }

    function test_BatchMintRewardsMaxBatch() public {
        uint256 n = token.MAX_BATCH() + 1;
        address[] memory r = new address[](n);
        uint256[] memory a = new uint256[](n);
        string[]  memory s = new string[](n);
        for (uint256 i = 0; i < n; i++) { r[i] = alice; a[i] = 1; s[i] = "c"; }
        vm.prank(minter);
        vm.expectRevert("Max 200 per batch");
        token.batchMintRewards(r, a, s);
    }

    function test_BatchMintRewardsEmitsEvent() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = alice; a[0] = 3e18; s[0] = "conversion";
        vm.expectEmit(true, false, false, true);
        emit YeetToken.RewardMinted(alice, 3e18, "conversion");
        vm.prank(minter);
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
        vm.prank(minter);
        token.batchMintRewards(r, a, s);
    }
}
