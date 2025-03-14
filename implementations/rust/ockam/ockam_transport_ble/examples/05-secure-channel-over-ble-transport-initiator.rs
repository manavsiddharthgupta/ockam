// This node routes a message, to a worker on a different node, over the ble transport.

use ockam::{
    identity::{Identity, TrustEveryonePolicy},
    route,
    vault::Vault,
    Context, Result,
};
use ockam_transport_ble::{BleTransport, BLE};

use ockam_transport_ble::driver::btleplug::BleAdapter;
use ockam_transport_ble::driver::BleClient;

#[ockam::node]
async fn main(mut ctx: Context) -> Result<()> {
    // Create a ble_client
    let ble_adapter = BleAdapter::try_new().await?;
    let ble_client = BleClient::with_adapter(ble_adapter);

    // Initialize the BLE Transport.
    let ble = BleTransport::create(&ctx).await?;

    // Create a Vault to safely store secret keys for Alice.
    let vault = Vault::create();

    // Create an Entity to represent Alice.
    let alice = Identity::create(&ctx, &vault).await?;

    // Connect to BLE Server
    ble.connect(ble_client, "ockam_ble_1".to_string()).await?;

    // Connect to a secure channel listener and perform a handshake.
    let r = route![(BLE, "ockam_ble_1"), "bob_listener"];
    let channel = alice.create_secure_channel(r, TrustEveryonePolicy).await?;

    // Send a message to the "echoer" worker, on a different node, via secure channel.
    let r = route![channel, "echoer"];
    ctx.send(r, "Hello Ockam!".to_string()).await?;

    // Wait to receive a reply and print it.
    let reply = ctx.receive::<String>().await?;
    println!("[main] App Received: {}", reply); // should print "Hello Ockam!"

    // Stop all workers, stop the node, cleanup and return.
    ctx.stop().await
}
