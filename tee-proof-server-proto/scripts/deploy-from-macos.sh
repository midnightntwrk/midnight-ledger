#!/bin/bash
# Deploy Midnight Proof Server to AWS Nitro from macOS
# This script handles all LOCAL steps (macOS) for deployment

set -e

echo "🚀 Midnight Proof Server - AWS Nitro Deployment (from macOS)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# Function to check prerequisites
check_prereqs() {
    echo -e "${BLUE}📋 Checking Prerequisites...${NC}"

    # Check AWS CLI
    if ! command -v aws &> /dev/null; then
        echo -e "${RED}❌ AWS CLI not found${NC}"
        echo "Install: brew install awscli"
        exit 1
    fi
    echo -e "${GREEN}✅ AWS CLI found${NC}"

    # Check jq
    if ! command -v jq &> /dev/null; then
        echo -e "${YELLOW}⚠️  jq not found - installing...${NC}"
        brew install jq
    fi
    echo -e "${GREEN}✅ jq found${NC}"

    # Check Docker (optional - only needed for save/transfer method)
    if command -v docker &> /dev/null; then
        echo -e "${GREEN}✅ Docker found${NC}"
    else
        echo -e "${YELLOW}⚠️  Docker not found (needed for save/transfer method)${NC}"
    fi

    echo ""
}

# Function to check AWS SSO login
check_sso_login() {
    echo -e "${BLUE}🔐 Checking AWS SSO Login...${NC}"

    if aws sts get-caller-identity &> /dev/null; then
        IDENTITY=$(aws sts get-caller-identity --query 'Arn' --output text)
        echo -e "${GREEN}✅ Logged in as: ${IDENTITY}${NC}"
        return 0
    else
        echo -e "${RED}❌ Not logged in to AWS SSO${NC}"
        echo ""
        echo "Please login with:"
        echo "  aws sso login --profile YOUR_PROFILE"
        echo ""
        echo "Or configure SSO:"
        echo "  aws configure sso"
        exit 1
    fi
}

# Gather configuration
gather_config() {
    echo ""
    echo -e "${BLUE}📝 Configuration${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # AWS Region
    read -p "AWS Region [us-east-1]: " REGION
    REGION=${REGION:-us-east-1}

    # SSH Key Name
    read -p "EC2 SSH Key Name: " KEY_NAME
    if [ -z "$KEY_NAME" ]; then
        echo -e "${RED}❌ SSH key name is required${NC}"
        exit 1
    fi

    # Instance Type
    read -p "Instance Type [c6i.2xlarge]: " INSTANCE_TYPE
    INSTANCE_TYPE=${INSTANCE_TYPE:-c6i.2xlarge}

    # Deployment method
    echo ""
    echo "Docker Image Deployment Method:"
    echo "  1) Build on EC2 (recommended - we'll guide you)"
    echo "  2) Save locally and transfer (requires Docker)"
    echo "  3) Use Docker registry (you handle push/pull)"
    read -p "Choice [1]: " DEPLOY_METHOD
    DEPLOY_METHOD=${DEPLOY_METHOD:-1}

    echo ""
    echo -e "${GREEN}Configuration:${NC}"
    echo "  Region: $REGION"
    echo "  SSH Key: $KEY_NAME"
    echo "  Instance Type: $INSTANCE_TYPE"
    echo "  Deploy Method: $DEPLOY_METHOD"
    echo ""

    read -p "Proceed? (yes/no) [yes]: " PROCEED
    PROCEED=${PROCEED:-yes}

    if [ "$PROCEED" != "yes" ]; then
        echo "Aborted."
        exit 0
    fi
}

# Launch EC2 instance
launch_instance() {
    echo ""
    echo -e "${BLUE}🚀 Launching EC2 Instance...${NC}"
    echo ""

    # Get latest Amazon Linux 2 AMI
    echo "Finding latest Amazon Linux 2 AMI..."
    AMI_ID=$(aws ec2 describe-images \
        --owners amazon \
        --filters "Name=name,Values=amzn2-ami-hvm-*-x86_64-gp2" \
                  "Name=state,Values=available" \
        --query 'Images | sort_by(@, &CreationDate) | [-1].ImageId' \
        --output text \
        --region $REGION)

    echo "  AMI: $AMI_ID"

    # Get your public IP
    MY_IP=$(curl -s https://checkip.amazonaws.com)
    echo "  Your IP: $MY_IP"

    # Create security group
    echo ""
    echo "Creating security group..."
    SG_ID=$(aws ec2 create-security-group \
        --group-name midnight-proof-server-sg-$(date +%s) \
        --description "Midnight Proof Server with Nitro Enclave" \
        --region $REGION \
        --query 'GroupId' \
        --output text)

    echo -e "${GREEN}✅ Security Group: $SG_ID${NC}"

    # Add rules
    echo "Adding security group rules..."
    aws ec2 authorize-security-group-ingress \
        --group-id $SG_ID \
        --protocol tcp \
        --port 22 \
        --cidr ${MY_IP}/32 \
        --region $REGION \
        --output text

    aws ec2 authorize-security-group-ingress \
        --group-id $SG_ID \
        --protocol tcp \
        --port 6300 \
        --cidr 0.0.0.0/0 \
        --region $REGION \
        --output text

    # Launch instance
    echo ""
    echo "Launching instance..."
    INSTANCE_ID=$(aws ec2 run-instances \
        --image-id $AMI_ID \
        --count 1 \
        --instance-type $INSTANCE_TYPE \
        --key-name $KEY_NAME \
        --security-group-ids $SG_ID \
        --enclave-options 'Enabled=true' \
        --block-device-mappings 'DeviceName=/dev/xvda,Ebs={VolumeSize=50,VolumeType=gp3}' \
        --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=midnight-proof-server}]' \
        --region $REGION \
        --query 'Instances[0].InstanceId' \
        --output text)

    echo -e "${GREEN}✅ Instance launched: $INSTANCE_ID${NC}"

    # Wait for running
    echo ""
    echo "Waiting for instance to start (this may take 1-2 minutes)..."
    aws ec2 wait instance-running --instance-ids $INSTANCE_ID --region $REGION

    # Get public IP
    PUBLIC_IP=$(aws ec2 describe-instances \
        --instance-ids $INSTANCE_ID \
        --query 'Reservations[0].Instances[0].PublicIpAddress' \
        --output text \
        --region $REGION)

    echo -e "${GREEN}✅ Instance is running!${NC}"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${GREEN}Instance Details:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Instance ID:  $INSTANCE_ID"
    echo "Public IP:    $PUBLIC_IP"
    echo "Security Group: $SG_ID"
    echo "Region:       $REGION"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Save details
    cat > ~/midnight-nitro-instance.txt << EOF
Instance ID: $INSTANCE_ID
Public IP: $PUBLIC_IP
Security Group: $SG_ID
Region: $REGION
SSH Key: $KEY_NAME
SSH Command: ssh -i ~/.ssh/${KEY_NAME}.pem ec2-user@${PUBLIC_IP}
Deployed: $(date)
EOF

    echo ""
    echo -e "${GREEN}✅ Instance details saved to: ~/midnight-nitro-instance.txt${NC}"
}

# Save and transfer Docker image
save_and_transfer() {
    echo ""
    echo -e "${BLUE}📦 Saving and Transferring Docker Image...${NC}"
    echo ""

    cd ~/code/midnight-code/midnight-ledger

    # Check if image exists
    if ! docker image inspect midnight/proof-server:latest &> /dev/null; then
        echo -e "${RED}❌ Docker image not found: midnight/proof-server:latest${NC}"
        echo ""
        echo "Please build the image first:"
        echo "  cd ~/code/midnight-code/midnight-ledger/tee-proof-server-proto"
        echo "  make build-local"
        exit 1
    fi

    echo "Saving Docker image (this may take a minute)..."
    docker save midnight/proof-server:latest | gzip > midnight-proof-server.tar.gz

    SIZE=$(ls -lh midnight-proof-server.tar.gz | awk '{print $5}')
    echo -e "${GREEN}✅ Image saved: $SIZE${NC}"

    echo ""
    echo "Transferring to EC2 (this may take a few minutes)..."
    PUBLIC_IP=$(cat ~/midnight-nitro-instance.txt | grep "Public IP" | cut -d' ' -f3)

    scp -i ~/.ssh/${KEY_NAME}.pem \
        -o StrictHostKeyChecking=no \
        midnight-proof-server.tar.gz \
        ec2-user@${PUBLIC_IP}:~/

    echo -e "${GREEN}✅ Transfer complete!${NC}"

    # Clean up local file
    rm midnight-proof-server.tar.gz
}

# Print next steps
print_next_steps() {
    PUBLIC_IP=$(cat ~/midnight-nitro-instance.txt | grep "Public IP" | cut -d' ' -f3)

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${GREEN}✅ EC2 Instance Ready!${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo -e "${YELLOW}📝 Next Steps (on EC2):${NC}"
    echo ""
    echo "1. SSH to the instance:"
    echo -e "   ${BLUE}ssh -i ~/.ssh/${KEY_NAME}.pem ec2-user@${PUBLIC_IP}${NC}"
    echo ""

    case $DEPLOY_METHOD in
        1)
            echo "2. Clone repo and build:"
            echo "   git clone https://github.com/your-org/midnight-code.git"
            echo "   cd midnight-code/midnight-ledger/tee-proof-server-proto"
            echo "   ./scripts/aws-nitro-deploy.sh --build"
            ;;
        2)
            echo "2. Load Docker image:"
            echo "   gunzip midnight-proof-server.tar.gz"
            echo "   docker load < midnight-proof-server.tar"
            echo ""
            echo "3. Deploy to Nitro:"
            echo "   git clone https://github.com/your-org/midnight-code.git"
            echo "   cd midnight-code/midnight-ledger/tee-proof-server-proto"
            echo "   ./scripts/aws-nitro-deploy.sh"
            ;;
        3)
            echo "2. Pull from registry:"
            echo "   docker pull YOUR_REGISTRY/midnight-proof-server:latest"
            echo "   docker tag YOUR_REGISTRY/midnight-proof-server:latest midnight/proof-server:latest"
            echo ""
            echo "3. Deploy to Nitro:"
            echo "   git clone https://github.com/your-org/midnight-code.git"
            echo "   cd midnight-code/midnight-ledger/tee-proof-server-proto"
            echo "   ./scripts/aws-nitro-deploy.sh"
            ;;
    esac

    echo ""
    echo -e "${YELLOW}📚 Full documentation:${NC}"
    echo "   ~/code/midnight-code/midnight-ledger/tee-proof-server-proto/AWS-NITRO-DEPLOYMENT.md"
    echo ""
    echo -e "${YELLOW}🧪 Test when deployed:${NC}"
    echo "   curl http://${PUBLIC_IP}:6300/health"
    echo ""
}

# Main execution
main() {
    check_prereqs
    check_sso_login
    gather_config
    launch_instance

    if [ "$DEPLOY_METHOD" = "2" ]; then
        save_and_transfer
    fi

    print_next_steps

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${GREEN}✅ Local setup complete!${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

main "$@"
