// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC721/ERC721.sol";
import "@openzeppelin/contracts/token/ERC721/extensions/ERC721URIStorage.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/**
 * @title YeetNFT
 * @notice BEP-721 contract for minting Yeet posts as NFTs on BSC.
 *
 * When a user mints a post as NFT:
 * - Post gets a permanent tokenId on-chain
 * - The 24h expiry is REMOVED (NFT posts live forever)
 * - Ownership is provably on BSC
 * - The metadata URI points to IPFS (content hash)
 */
contract YeetNFT is ERC721URIStorage, Ownable {

    uint256 private _nextTokenId;
    address public yeetToken;
    uint256 public mintFeeYeet = 5 * 10**18; // 5 YEET to mint

    // post_id (from DB, as bytes32) => tokenId
    mapping(bytes32 => uint256) public postToToken;
    // tokenId => post_id
    mapping(uint256 => bytes32) public tokenToPost;

    event PostMinted(
        address indexed author,
        uint256 indexed tokenId,
        bytes32 indexed postId,
        string ipfsUri
    );

    constructor(address _yeetToken) ERC721("Yeet Post", "YEET-NFT") Ownable(msg.sender) {
        yeetToken = _yeetToken;
    }

    /**
     * @notice Mint a Yeet post as an NFT.
     * @param postId   UUID of the post (as bytes32)
     * @param ipfsUri  IPFS URI of the post metadata (content + media)
     */
    function mintPost(
        bytes32 postId,
        string calldata ipfsUri
    ) external returns (uint256 tokenId) {
        require(postToToken[postId] == 0, "YeetNFT: post already minted");

        // Burn 5 YEET as mint fee
        IERC20Burnable(yeetToken).burnFrom(msg.sender, mintFeeYeet);

        tokenId = ++_nextTokenId;
        _mint(msg.sender, tokenId);
        _setTokenURI(tokenId, ipfsUri);

        postToToken[postId] = tokenId;
        tokenToPost[tokenId] = postId;

        emit PostMinted(msg.sender, tokenId, postId, ipfsUri);
    }

    function setMintFee(uint256 _fee) external onlyOwner {
        mintFeeYeet = _fee;
    }

    function totalMinted() external view returns (uint256) {
        return _nextTokenId;
    }
}

interface IERC20Burnable {
    function burnFrom(address account, uint256 amount) external;
}
