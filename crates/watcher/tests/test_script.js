// Helper script to deploy contract and make deposits for integration tests
// Run with: npx hardhat run tests/test_script.js --network localhost

const hre = require("hardhat");

async function main() {
  const [deployer, user] = await hre.ethers.getSigners();
  
  console.log("Deploying contracts...");
  const DepositContract = await hre.ethers.getContractFactory("DepositContract");
  const depositContract = await DepositContract.deploy();
  await depositContract.waitForDeployment();
  const depositAddress = await depositContract.getAddress();
  
  console.log("DepositContract deployed to:", depositAddress);
  console.log("TEST_DEPOSIT_CONTRACT_ADDRESS=" + depositAddress);
  
  // Make a test deposit
  console.log("\nMaking test deposit...");
  const assetId = 0; // Native ETH
  const amount = hre.ethers.parseEther("1.0");
  
  const tx = await depositContract.connect(user).depositNative(assetId, { value: amount });
  await tx.wait();
  
  console.log("Deposit transaction:", tx.hash);
  console.log("Test deposit completed");
}

main()
  .then(() => process.exit(0))
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });

