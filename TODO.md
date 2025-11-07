
Features:

- network model aware scheduling
-- during the host remodel, there are two types of network configurations
--- hosts with bridge per VXLAN
--- hosts with a single bridge with all V(X)LANS
-- group hosts by model
-- detect which model a VM uses
-- only try to schedule on matching pairs

- load aware scheduling
-- prometheus installation
-- new scheduler rules:
--- require enough RAM
--- then look for host with lowest CPU load

- image import
-- controller for image CRD
--- create new volume for image
--- download the image
---- can this be done straight to the ceph volume?
-- convert qcow2 to raw if necessary
--- this might need a temporary volume for the download

- make VM status reliable
-- currently nodes do not backfeed information about VMs running on them
-- vm status can get out of sync with reality
--- which fields the node controller owns, which fields the cluster controller owns
-- design a proper lifecycle graph for the VMs

- web UI? :D
