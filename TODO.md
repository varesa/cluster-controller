
Features:

# Load aware scheduling
- prometheus infrastructure installation
-- convert deployment to a helm chart
--- as a dependency add two prometheus replicas for HA
---- use local-path storageclass
--- add a thanos querier (2 replicas) to deduplicate the queries 

- new scheduler rules:
-- require enough RAM
-- then look for host with lowest CPU load

- resource reservations
-- resources consumed by VMs will not be immediately reflected in the metrics
-- if multiple VMs are scheduled at once, this may lead to oversubscription
-- make some sort of ephemeral reservations against hosts when scheduling VMs to avoid this issue

# Image import
- controller for image CRD
-- create new volume for image
-- download the image
--- can this be done straight to the ceph volume?
- convert qcow2 to raw if necessary
-- this might need a temporary volume for the download

# Make VM status reliable
- currently nodes do not backfeed information about VMs running on them
- vm status can get out of sync with reality
-- which fields the node controller owns, which fields the cluster controller owns
- design a proper lifecycle graph for the VMs

# CLI revamp

The command line usage is getting more complex and it probably makes sense 
to replace the hand-crafted argument parsing with 'clap'.

# Web UI? :D
