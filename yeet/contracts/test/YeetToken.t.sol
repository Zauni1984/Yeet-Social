// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../YeetToken.sol";
import "../YeetNFT.sol";

contract YeetTokenTest is Test {
    YeetToken token;
    YeetNFT   nft;

    address owner     = address(this);
    address team      = address(0x1);
    address ecosystem = address(0x2);
    address liquidity = address(0x3);
    address minter    = address(0x4);
    address user1     = address(0x5);
    address user2     = address(0x6);

    function setUp() public {
        token = new YeetToken(team, ecosystem, liquidity);
        token.setRewardsMinter(minter);
        nft   = new YeetNFT(address(token));
    }

    // ── Token supply ──────────────────────────────────────────────────────────

    function test_InitialSupply() public view {
        // Team (locked) + ecosystem + liquidity + reserve = 600M
        uint256 expected = 600_000_000 * 10**18;
        assertEq(token.totalSupply(), expected);
    }

    function test_MaxSupply() public view {
        assertEq(token.MAX_SUPPLY(), 1_000_000_000 * 10**18);
        assertEq(token.REWARDS_POOL(), 400_000_000 * 10**18);
    }

    // ── Batch rewards ─────────────────────────────────────────────────────────

    function test_BatchMintRewards() public {
        address[] memory recipients = new address[](2);
        uint256[] memory amounts    = new uint256[](2);
        string[]  memory actions    = new string[](2);

        recipients[0] = user1; amounts[0] = 1e18; actions[0] = "daily_login";
        recipients[1] = user2; amounts[1] = 5e17; actions[1] = "comment";

        vm.prank(minter);
        token.batchMintRewards(recipients, amounts, actions);

        assertEq(token.balanceOf(user1), 1e18);
        assertEq(token.balanceOf(user2), 5e17);
        assertEq(token.totalRewardsMinted(), 15e17);
    }

    function test_DailyCapEnforced() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = user1; s[0] = "gaming";

        // Mint 49 YEET — under cap
        a[0] = 49e18;
        vm.prank(minter);
        token.batchMintRewards(r, a, s);

        // Mint 2 more — total 51 > 50 cap, should revert
        a[0] = 2e18;
        vm.prank(minter);
        vm.expectRevert("YeetToken: daily reward limit reached");
        token.batchMintRewards(r, a, s);
    }

    function test_OnlyMinterCanMint() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = user1; a[0] = 1e18; s[0] = "login";

        vm.prank(user1);
        vm.expectRevert("YeetToken: not authorized minter");
        token.batchMintRewards(r, a, s);
    }

    // ── Tipping ───────────────────────────────────────────────────────────────

    function test_TipCreator() public {
        // Give user1 some tokens
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = user1; a[0] = 100e18; s[0] = "test";
        vm.prank(minter);
        token.batchMintRewards(r, a, s);

        // user1 tips user2 10 YEET
        vm.prank(user1);
        token.tipCreator(user2, 10e18);

        // user2 gets 9 YEET (10% platform fee)
        assertEq(token.balanceOf(user2), 9e18);
        // Owner (platform) gets 1 YEET fee
        assertEq(token.balanceOf(owner), 1e18 + 100_000_000e18); // reserve + fee
    }

    // ── NFT ───────────────────────────────────────────────────────────────────

    function test_MintNFT() public {
        // Give user1 enough YEET for mint fee (5 YEET)
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = user1; a[0] = 10e18; s[0] = "test";
        vm.prank(minter);
        token.batchMintRewards(r, a, s);

        // Approve NFT contract to burn
        vm.prank(user1);
        token.approve(address(nft), 5e18);

        // Mint post as NFT
        bytes32 postId = keccak256("post-uuid-123");
        vm.prank(user1);
        uint256 tokenId = nft.mintPost(postId, "ipfs://Qm...");

        assertEq(tokenId, 1);
        assertEq(nft.ownerOf(1), user1);
        assertEq(nft.postToToken(postId), 1);
        assertEq(token.balanceOf(user1), 5e18); // 10 - 5 burned
    }

    function test_CannotMintSamePostTwice() public {
        address[] memory r = new address[](1);
        uint256[] memory a = new uint256[](1);
        string[]  memory s = new string[](1);
        r[0] = user1; a[0] = 20e18; s[0] = "test";
        vm.prank(minter);
        token.batchMintRewards(r, a, s);

        vm.startPrank(user1);
        token.approve(address(nft), 20e18);

        bytes32 postId = keccak256("post-uuid-456");
        nft.mintPost(postId, "ipfs://Qm1");

        vm.expectRevert("YeetNFT: post already minted");
        nft.mintPost(postId, "ipfs://Qm2");
        vm.stopPrank();
    }

    // ── Vesting ───────────────────────────────────────────────────────────────

    function test_TeamTokensLockedUntilVesting() public {
        vm.prank(team);
        vm.expectRevert("YeetToken: vesting period not over");
        token.releaseTeamTokens();
    }

    function test_TeamTokensReleaseAfterVesting() public {
        vm.warp(block.timestamp + 731 days);
        vm.prank(team);
        token.releaseTeamTokens();
        assertEq(token.balanceOf(team), 150_000_000e18);
    }
}
