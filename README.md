# Honeypot Detector

A command-line interface application designed to analyze ERC20 tokens for potential honeypot characteristics. Implemented in Rust and leveraging the Revm for transaction simulation.

```
Usage: hp [OPTIONS] <TOKEN>

Arguments:
  <TOKEN>
          ERC20 token address to test

Options:
  -l, --logs
          Enable full logging
  -s, --sender <SENDER>
          Address from which the test will be done
  -r, --rpc-url <RPC_URL>
          The RPC endpoint. If no ETH_RPC_URL is set or no rpc_url is not passed, by default Flashbots RPC URL will be used [env: ETH_RPC_URL=http://192.168.0.212:8545/] [default: https://rpc.flashbots.net/fast]
  -h, --help
          Print help
  -V, --version
          Print version
```



## Features Checklist

- [x] Honeypot test on Uniswap V2
- [x] Option to specify sender address
- [x] Option to enable full logging
- [ ] Honeypot test on Uniswap V3
- [ ] Improved printouts in console
- [ ] More options such as token details (name, symbol, decimals, total supply), choose specific protocol to test against.