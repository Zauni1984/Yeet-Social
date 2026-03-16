// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/// @title YeetToken — BEP-20 utility token for Yeet Social on BSC
/// @notice 1 billion total supply, burnable, with platform mint reserve
contract YeetToken is ERC20, ERC20Burnable, ERC20Permit, Ownable {
    uint256 public constant MAX_SUPPLY = 1_000_000_000 * 10 ** 18; // 1 billion

    // Reward pool address (controlled by backend for engagement rewards)
    address public rewardPool;

    event RewardPoolUpdated(address indexed oldPool, address indexed newPool);

    constructor(address initialOwner, address _rewardPool)
        ERC20("Yeet Token", "YEET")
        ERC20Permit("Yeet Token")
        Ownable(initialOwner)
    {
        require(_rewardPool != address(0), "Invalid reward pool");
        rewardPool = _rewardPool;

        // Distribution at launch:
        // 40% — community / airdrop reserve (owner holds)
        // 30% — reward pool (engagement rewards)
        // 20% — team & development (vested, owner holds)
        // 10% — liquidity (owner holds)
        uint256 communityShare  = (MAX_SUPPLY * 40) / 100;
        uint256 rewardShare     = (MAX_SUPPLY * 30) / 100;
        uint256 teamShare       = (MAX_SUPPLY * 20) / 100;
        uint256 liquidityShare  = (MAX_SUPPLY * 10) / 100;

        _mint(initialOwner,  communityShare + teamShare + liquidityShare);
        _mint(_rewardPool,   rewardShare);
    }

    /// @notice Update reward pool address (only owner)
    function setRewardPool(address _newPool) external onlyOwner {
        require(_newPool != address(0), "Invalid address");
        emit RewardPoolUpdated(rewardPool, _newPool);
        rewardPool = _newPool;
    }

    /// @notice Mint additional tokens — capped at MAX_SUPPLY
    function mint(address to, uint256 amount) external onlyOwner {
        require(totalSupply() + amount <= MAX_SUPPLY, "Exceeds max supply");
        _mint(to, amount);
    }

    /// @notice Decimals override (18, same as ETH)
    function decimals() public pure override returns (uint8) {
        return 18;
    }
}