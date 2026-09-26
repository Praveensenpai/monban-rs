#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl BoundingBox {
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    pub fn area(&self) -> f32 {
        let width = (self.x2 - self.x1).max(0.0);
        let height = (self.y2 - self.y1).max(0.0);
        width * height
    }

    pub fn iou(&self, other: &Self) -> f32 {
        let inter_x1 = self.x1.max(other.x1);
        let inter_y1 = self.y1.max(other.y1);
        let inter_x2 = self.x2.min(other.x2);
        let inter_y2 = self.y2.min(other.y2);

        let inter_width = (inter_x2 - inter_x1).max(0.0);
        let inter_height = (inter_y2 - inter_y1).max(0.0);
        let inter_area = inter_width * inter_height;

        let union_area = self.area() + other.area() - inter_area;
        if union_area <= 0.0 {
            0.0
        } else {
            inter_area / union_area
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub class_id: usize,
    pub label: String,
    pub confidence: f32,
    pub box_coords: BoundingBox,
}

pub const COCO_CLASSES: [&str; 80] = [
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic light",
    "fire hydrant",
    "stop sign",
    "parking meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "backpack",
    "umbrella",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports ball",
    "kite",
    "baseball bat",
    "baseball glove",
    "skateboard",
    "surfboard",
    "tennis racket",
    "bottle",
    "wine glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted plant",
    "bed",
    "dining table",
    "toilet",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy bear",
    "hair drier",
    "toothbrush",
];

pub fn class_emoji(label: &str) -> &'static str {
    match label {
        "person" => "👤",
        "dog" => "🐕",
        "cat" => "🐈",
        "cow" => "🐄",
        "bird" => "🐦",
        "horse" => "🐎",
        "sheep" => "🐑",
        "bear" => "🐻",
        "elephant" => "🐘",
        "car" | "truck" | "bus" => "🚗",
        "motorcycle" | "bicycle" => "🏍️",
        "laptop" | "tv" => "💻",
        "cell phone" => "📱",
        _ => "🎯",
    }
}
