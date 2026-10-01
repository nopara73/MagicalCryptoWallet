# Coordinator container

From the repository root, build `docker build -f Contrib/Containers/Dockerfile -t magicalcryptowallet-coordinator .`. The image builds native credentials from the pinned source, runs their tests, and publishes the current coordinator.

Configure an isolated data volume, your Bitcoin RPC connection, the intended network, and the listen address before exposing port 38126. Fee collection is disabled until you provide your own xpub and enable it. The retired indexer backend is not a supported container target.
