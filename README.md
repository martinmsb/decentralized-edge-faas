# decentralized-edge-faas

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

