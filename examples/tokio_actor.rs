use rosemary::observability::init_tracing;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info};

/// CounterCommand enum for interacting with the CounterActor.
#[derive(Debug)]
enum CounterCommand {
    /// Increment the internal counter.
    Increment,
    /// Get the current count value.
    GetCount(oneshot::Sender<u64>),
}

/// CounterActor holds the internal state and processes commands.
struct CounterActor {
    count: u64,
    receiver: mpsc::Receiver<CounterCommand>,
}

impl CounterActor {
    /// Creates a new CounterActor with the given receiver.
    fn new(receiver: mpsc::Receiver<CounterCommand>) -> Self {
        Self { count: 0, receiver }
    }

    /// The main actor loop that processes incoming commands.
    async fn run(mut self) {
        info!("CounterActor loop started");
        while let Some(command) = self.receiver.recv().await {
            match command {
                CounterCommand::Increment => {
                    self.count += 1;
                    debug!(count = self.count, "Incremented count");
                }
                CounterCommand::GetCount(reply) => {
                    debug!(count = self.count, "Getting count");
                    // Send the count back, ignoring errors if the receiver dropped.
                    let _ = reply.send(self.count);
                }
            }
        }
        info!("CounterActor loop finished");
    }
}

/// CounterHandle provides a thread-safe handle to interact with the CounterActor.
#[derive(Clone, Debug)]
pub struct CounterHandle {
    sender: mpsc::Sender<CounterCommand>,
}

impl CounterHandle {
    /// Spawns a new CounterActor and returns a handle to it.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(32);
        let actor = CounterActor::new(receiver);

        // Spawn the actor's run loop in a background task.
        tokio::spawn(async move {
            actor.run().await;
        });

        Self { sender }
    }
}

impl Default for CounterHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterHandle {
    /// Sends an increment command to the actor.
    pub async fn increment(&self) -> anyhow::Result<()> {        self.sender
            .send(CounterCommand::Increment)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send increment command: {}", e))
    }

    /// Requests the current count from the actor.
    pub async fn get_count(&self) -> anyhow::Result<u64> {
        let (reply_sender, reply_receiver) = oneshot::channel();
        self.sender
            .send(CounterCommand::GetCount(reply_sender))
            .await
            .map_err(|e| anyhow::anyhow!("failed to send get_count command: {}", e))?;

        reply_receiver
            .await
            .map_err(|e| anyhow::anyhow!("failed to receive count from actor: {}", e))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    init_tracing();

    info!("Starting tokio actor pattern example...");

    // Create a new CounterHandle (this also spawns the actor)
    let handle = CounterHandle::new();

    // Increment several times
    info!("Incrementing counter 3 times...");
    handle.increment().await?;
    handle.increment().await?;
    handle.increment().await?;

    // Get the current count
    let count = handle.get_count().await?;
    info!(count = count, "Final count received from actor");

    assert_eq!(count, 3);
    info!("Example completed successfully!");

    Ok(())
}
