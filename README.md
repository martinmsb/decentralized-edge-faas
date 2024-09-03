# Decentralized Edge FaaS

This repository contains the code for the Final Master Project: Decentralzied Edge FaaS.

OpenFaaS must be installed as explained in [OpenFaaS installation](#openfaas-installation).

## Code execution

To bootstrap a node to initiate the P2P network, run the following command:
```console
cargo run -- --p2p-listen-address /ip4/<ip-address>/tcp/<tcp-port>
    --secret-key-seed 1
    --http-listen-port 8000
    --docker-username <docker-hub-username>
```

To connect a new Peer to an existing one, run the following command:
```console
cargo run --p2p-listen-address /ip4/<new-node-ip-address>/tcp/<new-node-tcp-port>
    --peer ip4/<exising-node-ip-address>/tcp/<existing-node-tcp-port>/p2p/12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X
    --http-listen-port 8000
    --docker-username <docker-hub-username>
```

Every node exposes an HTTP server wtih the following endpoints to test all the features implemented:
- **POST /functions/deployments**: Deploy a new function. A Multipart form with the following fields is required:
    - *handler*: A handler\.py python file with the handler code for the function.
    - *requirements*: A requirements.txt file with the dependencies for the function.
- **POST /functions/{function_name}/executions**: Execute a function. Function name is required as path parameter. Body JSON with the following fields is required:
    - *http_method*: HTTP method to use in the function request.
    - *path_and_query* (optional): Path and query to use in the function request.
    - *body* (optional): Body to use in the function request.
- **PUT /functions/deployments/{function_name}**: Deploy an existing function in the network but
in a different node (or update the function where in the deployed node). A Multipart form with the following fields is required:
    - *handler*: A handler\.py python file with the handler code for the function.
    - *requirements*: A requirements.txt file with the dependencies for the function.
- **POST /functions/{function_name}/executions/manycall**: Execute different invocations of the same function. Function name is required as path parameter. Body JSON with the following field is required:
    - *items*: List of items to execute the function with. Each item corresponds to the body which will be sent in each function invocation.


## OpenFaaS installation

The only previous requirement is to have Docker installed in the system. 
Make sure the user you are logged in is added to docker group in order to have privileges to run Docker commands.
Log into the default Docker Hub repository with your account to enable updating funciton images.

1. Install arkade as a tool to install and deploy apps and servited to Kubernetes:
```console
$ curl -sLS https://get.arkade.dev | sudo sh
```

2. Install kubectl to run commands against Kubernetes clusters:
```console
$ arkade get kubectl
```

3. Install KinD to create a Kubernetes cluster in Docker:
```console
$ curl -Lo ./kind https://kind.sigs.k8s.io/dl/v0.15.0/kind-linux-amd64
$ chmod +x ./kind
$ sudo mv ./kind /usr/local/bin/kind
```

4. Create KinD cluster:
```console
$ kind create cluster
```
Check your cluster is up:
```console
$ kubectl config use kind-kind
$ kubectl cluster-info
```

5. Install OpenFaaS which is deployed to the KinD cluster:
```console
$ arkade install openfaas
```
Check that OpenFaaS is deployed
```console
$ kubectl get pods -n openfaas
```
Install faas-cli:
```console
$ curl -SLsf https://cli.openfaas.com | sudo sh
```

6. Forward FaaS gateway to the system:
```console
$ kubectl rollout status -n openfaas deploy/gateway
$ kubectl port-forward -n openfaas svc/gateway 8080:8080 &
```

7. Setup basic auth to log into the gateway:
```console
$ PASSWORD=$(kubectl get secret -n openfaas basic-auth -o jsonpath="{.data.basic-auth-password}" | base64 --decode; echo)
$ echo -n $PASSWORD | faas-cli login --username admin --password-stdin
```
Store the password obtained by:
```console
$ echo $PASSWORD
```

