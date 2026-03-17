// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Test.sol";
import "../src/YeetNFT.sol";

contract YeetNFTTest is Test {
    YeetNFT nft;
    address owner;
    address alice = makeAddr("alice");
    address bob   = makeAddr("bob");

    bytes32 POST_ID  = keccak256("post-abc");

    function setUp() public {
        owner = makeAddr("owner");
        vm.prank(owner);
        nft = new YeetNFT(owner);
    }

    function test_MintPost() public {
        vm.prank(alice);
        uint256 tokenId = nft.mintPost(POST_ID, "ipfs://Qm123");
        assertEq(nft.ownerOf(tokenId), alice);
        assertEq(nft.author(tokenId),  alice);
        assertTrue(nft.postMinted(POST_ID));
    }

    function test_CannotMintSamePostTwice() public {
        vm.startPrank(alice);
        nft.mintPost(POST_ID, "ipfs://Qm123");
        vm.expectRevert("Post already minted");
        nft.mintPost(POST_ID, "ipfs://Qm123");
        vm.stopPrank();
    }

    function test_TokenURI() public {
        vm.prank(alice);
        uint256 tokenId = nft.mintPost(POST_ID, "ipfs://QmABC");
        assertEq(nft.tokenURI(tokenId), "ipfs://QmABC");
    }

    function test_RoyaltyInfo() public {
        vm.prank(alice);
        uint256 tokenId = nft.mintPost(POST_ID, "ipfs://QmABC");
        (address receiver, uint256 royaltyAmt) = nft.royaltyInfo(tokenId, 10000);
        assertEq(receiver,   alice);
        assertEq(royaltyAmt, 1000);
    }

    function test_MintFeeEnforced() public {
        vm.prank(owner);
        nft.setMintFee(0.01 ether);
        vm.deal(alice, 1 ether);
        vm.prank(alice);
        vm.expectRevert("Insufficient mint fee");
        nft.mintPost(POST_ID, "ipfs://Qm");
    }

    function test_WithdrawFee() public {
        vm.prank(owner);
        nft.setMintFee(0.01 ether);
        vm.deal(alice, 1 ether);
        vm.prank(alice);
        nft.mintPost{value: 0.01 ether}(POST_ID, "ipfs://Qm");
        assertEq(address(nft).balance, 0.01 ether);
        uint256 ownerBefore = owner.balance;
        vm.prank(owner);
        nft.withdraw();
        assertEq(owner.balance, ownerBefore + 0.01 ether);
    }
}
