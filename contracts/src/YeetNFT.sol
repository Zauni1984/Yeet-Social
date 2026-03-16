// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721Royalty.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/// @title YeetNFT — Posts minted as NFTs on BSC
/// @notice Each NFT represents a Yeet post; creator retains royalties
contract YeetNFT is ERC721, ERC721URIStorage, ERC721Royalty, Ownable {
    uint256 private _nextTokenId;

    // Mint fee in BNB (can be 0 for free minting)
    uint256 public mintFee;

    // postId => tokenId (prevent double-minting same post)
    mapping(bytes32 => uint256) public postTokenId;
    mapping(bytes32 => bool)    public postMinted;

    // tokenId => original author
    mapping(uint256 => address) public author;

    event PostMinted(
        address indexed creator,
        bytes32 indexed postId,
        uint256 indexed tokenId,
        string  tokenURI
    );

    event MintFeeUpdated(uint256 oldFee, uint256 newFee);

    constructor(address initialOwner)
        ERC721("Yeet NFT", "YNFT")
        Ownable(initialOwner)
    {
        mintFee = 0; // free on testnet
        // Default royalty: 5% to contract owner (platform)
        _setDefaultRoyalty(initialOwner, 500);
    }

    /// @notice Mint a Yeet post as an NFT
    /// @param postId Off-chain UUID of the post (bytes32)
    /// @param uri   IPFS/Arweave metadata URI
    function mintPost(bytes32 postId, string calldata uri)
        external
        payable
        returns (uint256)
    {
        require(!postMinted[postId], "Post already minted");
        require(msg.value >= mintFee, "Insufficient mint fee");

        uint256 tokenId = _nextTokenId++;
        _safeMint(msg.sender, tokenId);
        _setTokenURI(tokenId, uri);

        // Creator gets 10% royalty on secondary sales
        _setTokenRoyalty(tokenId, msg.sender, 1000);

        postTokenId[postId] = tokenId;
        postMinted[postId]  = true;
        author[tokenId]     = msg.sender;

        emit PostMinted(msg.sender, postId, tokenId, uri);
        return tokenId;
    }

    // ── Admin ──────────────────────────────────────────────────

    function setMintFee(uint256 _fee) external onlyOwner {
        emit MintFeeUpdated(mintFee, _fee);
        mintFee = _fee;
    }

    function withdraw() external onlyOwner {
        payable(owner()).transfer(address(this).balance);
    }

    // ── Overrides ──────────────────────────────────────────────

    function tokenURI(uint256 tokenId)
        public view override(ERC721, ERC721URIStorage) returns (string memory)
    {
        return super.tokenURI(tokenId);
    }

    function supportsInterface(bytes4 interfaceId)
        public view override(ERC721, ERC721URIStorage, ERC721Royalty) returns (bool)
    {
        return super.supportsInterface(interfaceId);
    }
}