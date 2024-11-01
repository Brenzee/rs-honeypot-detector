// // sqrtPriceLimitX96 == 0
// //     ? (zeroForOne ? TickMath.MIN_SQRT_RATIO + 1 : TickMath.MAX_SQRT_RATIO - 1)
// //     : sqrtPriceLimitX96,

// // /// @dev The minimum tick that may be passed to #getSqrtRatioAtTick computed from log base 1.0001 of 2**-128
// // int24 internal constant MIN_TICK = -887272;
// // /// @dev The maximum tick that may be passed to #getSqrtRatioAtTick computed from log base 1.0001 of 2**128
// // int24 internal constant MAX_TICK = -MIN_TICK;

// use alloy::primitives::aliases::U24;
// use alloy::primitives::U160;
// use alloy::sol;
// use alloy::sol_types::{SolCall, SolValue};
// use anyhow::{anyhow, Result};
// use revm::primitives::{address, keccak256, AccountInfo, Address, ExecutionResult, TxKind, U256};
// use revm::Evm;

// use crate::cli::DEFAULT_ACC;
// use crate::revm_actions::balance_of;
// use crate::{AlloyCacheDB, WETH};

// const UNIV3_ROUTER: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");

// sol! {
//     function swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data) external returns (int256 amount0, int256 amount1);

//     struct ExactInputSingleParams {
//         address tokenIn;
//         address tokenOut;
//         uint24 fee;
//         address recipient;
//         uint256 deadline;
//         uint256 amountIn;
//         uint256 amountOutMinimum;
//         uint160 sqrtPriceLimitX96;
//     }
//     function exactInputSingle(ExactInputSingleParams calldata params) external returns (uint256 amountOut);

//     function approve(address spender, uint256 amount) external returns (bool);
// }

// pub fn test_swap(db: &mut AlloyCacheDB) -> Result<()> {
//     let sender = DEFAULT_ACC;
//     // USDC/WETH
//     let weth_usdc_500_pool = address!("88e6a0c2ddd26feeb64f039a2c41296fcb3f5640");
//     let usdc = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
//     // Swap from WETH to USDC
//     let zero_for_one = false;
//     // 0.1 WETH
//     // let amount_specified = I256::try_from(1 * 10u128.pow(18) / 10).unwrap();
//     let amount_specified = U256::try_from(1 * 10u128.pow(18) / 10).unwrap();

//     // Adding WETH to sender
//     let weth_balance_slot = U256::from(3);
//     let one_eth = U256::from(10_u128.pow(18));
//     let weth_user_balance_slot = keccak256((sender, weth_balance_slot).abi_encode());
//     db.insert_account_storage(WETH, weth_user_balance_slot.into(), one_eth)
//         .expect("Failed to insert account storage");
//     db.insert_account_info(
//         sender,
//         AccountInfo {
//             balance: one_eth,
//             ..Default::default()
//         },
//     );

//     // transfer(sender, weth_usdc_500_pool, one_eth, WETH, db)?;
//     approve(WETH, sender, one_eth, db)?;

//     univ3_swap(
//         weth_usdc_500_pool,
//         usdc,
//         sender,
//         zero_for_one,
//         amount_specified,
//         db,
//     )?;

//     let usdc_balance_after = balance_of(usdc, sender, sender, db)?;
//     println!("USDC balance after swap: {}", usdc_balance_after);

//     Ok(())
// }

// fn univ3_swap(
//     pool: Address,
//     token: Address,
//     sender: Address,
//     zero_for_one: bool,
//     amount_specified: U256,
//     db: &mut AlloyCacheDB,
// ) -> Result<()> {
//     let min_sqrt_ratio: U160 = U160::from(4295128739_u64);
//     let max_sqrt_ratio: U160 = "1461446703485210103287273052203988822378723970342"
//         .parse()
//         .unwrap();

//     let sqrt_price_limit_x96 = if zero_for_one {
//         min_sqrt_ratio.checked_add(U160::from(1)).unwrap()
//     } else {
//         max_sqrt_ratio.checked_sub(U160::from(1)).unwrap()
//     };

//     // let amount_specified = if zero_for_one {
//     //     amount_specified
//     // } else {
//     //     -amount_specified
//     // };

//     println!("sqrt_price_limit_x96: {}", sqrt_price_limit_x96);
//     println!("amount_specified: {}", amount_specified);

//     // let calldata = swapCall {
//     //     recipient: DEFAULT_ACC,
//     //     zeroForOne: zero_for_one,
//     //     amountSpecified: amount_specified,
//     //     sqrtPriceLimitX96: sqrt_price_limit_x96,
//     //     data: Bytes::default(),
//     // }
//     // .abi_encode();
//     let params = ExactInputSingleParams {
//         tokenIn: WETH,
//         tokenOut: token,
//         fee: U24::from(500),
//         recipient: sender,
//         deadline: U256::from(1111111111),
//         amountIn: amount_specified,
//         amountOutMinimum: U256::from(0),
//         // sqrtPriceLimitX96: sqrt_price_limit_x96,
//         sqrtPriceLimitX96: U160::from(0),
//     };

//     let calldata = exactInputSingleCall { params }.abi_encode();

//     let token_balance_before = balance_of(token, sender, sender, db)?;
//     println!("USDC balance before swap: {}", token_balance_before);

//     let mut evm = Evm::builder()
//         .with_db(db)
//         .modify_tx_env(|tx| {
//             tx.caller = sender;
//             tx.transact_to = TxKind::Call(UNIV3_ROUTER);
//             tx.data = calldata.into();
//             // tx.gas_limit = 100_000_000;
//         })
//         .build();

//     let tx = evm
//         .transact_commit()
//         .map_err(|e| anyhow!("Failed to execute swap call on Uniswap V3 pair: {e}"))?;

//     match tx {
//         ExecutionResult::Success { .. } => Ok(()),
//         result => {
//             println!("Swap failed: {:?}", result);
//             return Err(anyhow!(
//                 "'swap' execution failed on Uniswap V2 pair: {result:?}"
//             ));
//         }
//     }
// }

// fn approve(token: Address, sender: Address, amount: U256, db: &mut AlloyCacheDB) -> Result<()> {
//     let calldata = approveCall {
//         spender: UNIV3_ROUTER,
//         amount,
//     }
//     .abi_encode();

//     let mut evm = Evm::builder()
//         .with_db(db)
//         .modify_tx_env(|tx| {
//             tx.caller = sender;
//             tx.transact_to = TxKind::Call(token);
//             tx.data = calldata.into();
//         })
//         .build();

//     let tx = evm
//         .transact_commit()
//         .map_err(|e| anyhow!("Failed to execute transfer call: {e}"))?;

//     match tx {
//         ExecutionResult::Success { .. } => return Ok(()),
//         result => return Err(anyhow!("'approve' execution failed: {result:?}")),
//     };
// }
